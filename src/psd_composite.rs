//! Find the saved Photoshop composite without reading the layer payload.
use flate2::{Decompress, FlushDecompress, Status};
use image::metadata::Orientation;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_PREFIX: u64 = 64 * 1024 * 1024;
const MAX_THUMBNAIL: u64 = 2 * 1024 * 1024;
const MAX_ICC: u64 = 4 * 1024 * 1024;

pub(crate) struct Info {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) version: u16,
    pub(crate) channels: u16,
    pub(crate) depth: u16,
    pub(crate) mode: u16,
    pub(crate) has_real_merged: bool,
    pub(crate) compression: u16,
    pub(crate) composite_offset: u64,
    pub(crate) thumbnail: Option<Vec<u8>>,
    pub(crate) icc: Option<Vec<u8>>,
    pub(crate) orientation: Orientation,
    pub(crate) layer_bytes: u64,
    pub(crate) layer_count: Option<u16>,
    pub(crate) layer_pixels: Option<u64>,
    layers_offset: u64,
    alpha_marker: Option<i16>,
}

pub(crate) struct EmbeddedThumbnail {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) has_real_merged: bool,
    pub(crate) jpeg: Vec<u8>,
    pub(crate) orientation: Orientation,
}

pub(crate) fn inspect(path: &Path) -> Result<Info, String> {
    inspect_file(File::open(path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("PSD/PSB 合成图索引无效：{error}"))
}

pub(crate) fn embedded_thumbnail(path: &Path, cancel: &AtomicBool) -> Option<EmbeddedThumbnail> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let end = metadata.len();
    if !metadata.is_file() || end > crate::imaging::input_byte_limit(&path.to_string_lossy()) {
        return None;
    }
    let mut input = BufReader::new(file);
    let header = read_header(&mut input).ok()?;
    let (resources, _) = read_resources(&mut input, end, Some(cancel)).ok()?;
    Some(EmbeddedThumbnail {
        width: header.width,
        height: header.height,
        has_real_merged: resources.has_real_merged,
        jpeg: resources.thumbnail?,
        orientation: resources.orientation.unwrap_or(Orientation::NoTransforms),
    })
}

struct Header {
    version: u16,
    channels: u16,
    width: u32,
    height: u32,
    depth: u16,
    mode: u16,
}

fn read_header(input: &mut impl Read) -> io::Result<Header> {
    let header = read::<26>(input)?;
    let version = u16::from_be_bytes([header[4], header[5]]);
    if &header[..4] != b"8BPS" || !matches!(version, 1 | 2) {
        return Err(invalid("invalid Photoshop header"));
    }
    let channels = u16::from_be_bytes([header[12], header[13]]);
    let height = u32::from_be_bytes(header[14..18].try_into().unwrap());
    let width = u32::from_be_bytes(header[18..22].try_into().unwrap());
    let depth = u16::from_be_bytes([header[22], header[23]]);
    let mode = u16::from_be_bytes([header[24], header[25]]);
    let side = if version == 1 { 30_000 } else { 300_000 };
    if !(1..=56).contains(&channels)
        || width == 0
        || height == 0
        || width.max(height) > side
        || !matches!(depth, 1 | 8 | 16 | 32)
    {
        return Err(invalid("invalid canvas size, channels or bit depth"));
    }
    Ok(Header {
        version,
        channels,
        width,
        height,
        depth,
        mode,
    })
}

struct Resources {
    has_real_merged: bool,
    thumbnail: Option<Vec<u8>>,
    icc: Option<Vec<u8>>,
    orientation: Option<Orientation>,
}

fn read_resources(
    input: &mut BufReader<File>,
    end: u64,
    preview_cancel: Option<&AtomicBool>,
) -> io::Result<(Resources, u64)> {
    let color_length = u64::from(u32::from_be_bytes(read(input)?));
    let after_color = checked_end(input.stream_position()?, color_length, end.min(MAX_PREFIX))?;
    input.seek(SeekFrom::Start(after_color))?;
    let resources_length = u64::from(u32::from_be_bytes(read(input)?));
    let layers_offset = checked_end(
        input.stream_position()?,
        resources_length,
        end.min(MAX_PREFIX),
    )?;
    let mut resources = Resources {
        has_real_merged: true,
        thumbnail: None,
        icc: None,
        orientation: None,
    };
    let blocks = if preview_cancel.is_some() {
        4096
    } else {
        65_536
    };
    for _ in 0..blocks {
        if preview_cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            return Err(io::Error::other("cancelled"));
        }
        let start = input.stream_position()?;
        if start == layers_offset {
            break;
        }
        checked_end(start, 12, layers_offset)?;
        if &read::<4>(input)? != b"8BIM" {
            return Err(invalid("invalid image resource signature"));
        }
        let id = u16::from_be_bytes(read(input)?);
        let name_length = u64::from(read::<1>(input)?[0]);
        let name_padding = u64::from(name_length % 2 == 0);
        let length_offset = checked_end(
            input.stream_position()?,
            name_length + name_padding,
            layers_offset,
        )?;
        checked_end(length_offset, 4, layers_offset)?;
        input.seek_relative((name_length + name_padding) as i64)?;
        let length = u64::from(u32::from_be_bytes(read(input)?));
        let next = checked_end(input.stream_position()?, length + length % 2, layers_offset)?;
        match id {
            1057 if length >= 5 => resources.has_real_merged &= read::<5>(input)?[4] != 0,
            1036 if resources.thumbnail.is_none()
                && (28..=MAX_THUMBNAIL + 28).contains(&length) =>
            {
                resources.thumbnail = read_thumbnail(input, length)?;
            }
            1039 if preview_cancel.is_none()
                && resources.icc.is_none()
                && (1..=MAX_ICC).contains(&length) =>
            {
                let mut bytes = vec![0; length as usize];
                input.read_exact(&mut bytes)?;
                resources.icc = Some(bytes);
            }
            1058 | 1059 if resources.orientation.is_none() && (8..=MAX_ICC).contains(&length) => {
                let mut bytes = vec![0; length as usize];
                input.read_exact(&mut bytes)?;
                resources.orientation =
                    Orientation::from_exif_chunk(bytes.strip_prefix(b"Exif\0\0").unwrap_or(&bytes));
            }
            _ => {}
        }
        let remaining = (next - input.stream_position()?) as i64;
        input.seek_relative(remaining)?;
    }
    if input.stream_position()? != layers_offset {
        return Err(invalid("too many image resources"));
    }
    Ok((resources, layers_offset))
}

fn inspect_file(file: File) -> io::Result<Info> {
    let end = file.metadata()?.len();
    let mut input = BufReader::new(file);
    let Header {
        version,
        channels,
        width,
        height,
        depth,
        mode,
    } = read_header(&mut input)?;
    let (resources, layers_offset) = read_resources(&mut input, end, None)?;
    let length = read_layer_length(&mut input, version)?;
    let start = input.stream_position()?;
    let composite_offset = checked_end(start, length, end)?;
    let layers = if length > 0 {
        layer_info(
            &mut input,
            version,
            composite_offset,
            !resources.has_real_merged,
        )
        .ok()
        .flatten()
    } else {
        None
    };
    let alpha_marker = layers.as_ref().and_then(|layers| layers.alpha_marker);
    input.seek(SeekFrom::Start(composite_offset))?;
    let compression = match read(&mut input) {
        Ok(bytes) => u16::from_be_bytes(bytes),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => u16::MAX,
        Err(error) => return Err(error),
    };
    Ok(Info {
        width,
        height,
        version,
        channels,
        depth,
        mode,
        has_real_merged: resources.has_real_merged,
        compression,
        composite_offset,
        thumbnail: resources.thumbnail,
        icc: resources.icc,
        orientation: resources.orientation.unwrap_or(Orientation::NoTransforms),
        layer_bytes: length,
        layer_count: layers.as_ref().and_then(|layers| layers.count),
        layer_pixels: layers.and_then(|layers| layers.pixels),
        layers_offset,
        alpha_marker,
    })
}

fn read_thumbnail(input: &mut impl Read, length: u64) -> io::Result<Option<Vec<u8>>> {
    let header = read::<28>(input)?;
    let field = |offset| u32::from_be_bytes(header[offset..offset + 4].try_into().unwrap());
    let (format, width, height, compressed) = (field(0), field(4), field(8), u64::from(field(20)));
    if format != 1
        || width == 0
        || height == 0
        || width.max(height) > 2048
        || !(3..=MAX_THUMBNAIL).contains(&compressed)
        || compressed != length - 28
        || header[24..] != [0, 24, 0, 1]
    {
        return Ok(None);
    }
    let mut data = vec![0; compressed as usize];
    input.read_exact(&mut data)?;
    Ok(data.starts_with(&[0xff, 0xd8, 0xff]).then_some(data))
}

struct LayerInfo {
    alpha_marker: Option<i16>,
    count: Option<u16>,
    pixels: Option<u64>,
}

fn layer_info(
    input: &mut (impl Read + Seek),
    version: u16,
    end: u64,
    measure_pixels: bool,
) -> io::Result<Option<LayerInfo>> {
    let length_bytes = u64::from(version) * 4;
    checked_end(input.stream_position()?, length_bytes, end)?;
    let info_length = read_layer_length(input, version)?;
    let after_info = checked_end(input.stream_position()?, info_length, end)?;
    if info_length >= 2 {
        return read_layer_info(input, version, after_info, false, measure_pixels).map(Some);
    }
    if checked_end(after_info, 4, end).is_err() {
        return Ok(None);
    }
    input.seek(SeekFrom::Start(after_info))?;
    let mask_size = read::<4>(input)?;
    if matches!(&mask_size, b"8BIM" | b"8B64") {
        input.seek(SeekFrom::Start(after_info))?;
    } else {
        let mask_length = u64::from(u32::from_be_bytes(mask_size));
        let after_mask = checked_end(input.stream_position()?, mask_length, end)?;
        input.seek(SeekFrom::Start(after_mask))?;
    }

    // For high-bit-depth documents Photoshop can store the layer count in a
    // global tagged block instead of the main Layer Info section.
    let mut transparency = false;
    for _ in 0..256 {
        let offset = input.stream_position()?;
        if checked_end(offset, 12, end).is_err() {
            break;
        }
        let signature = read::<4>(input)?;
        if !matches!(&signature, b"8BIM" | b"8B64") {
            break;
        }
        let key = read::<4>(input)?;
        let wide = &signature == b"8B64"
            || version == 2
                && matches!(
                    &key,
                    b"LMsk"
                        | b"Lr16"
                        | b"Lr32"
                        | b"Layr"
                        | b"Mt16"
                        | b"Mt32"
                        | b"Mtrn"
                        | b"Alph"
                        | b"FMsk"
                        | b"lnk2"
                        | b"FEid"
                        | b"FXid"
                        | b"PxSD"
                );
        checked_end(input.stream_position()?, if wide { 8 } else { 4 }, end)?;
        let length = read_layer_length(input, if wide { 2 } else { 1 })?;
        let payload = input.stream_position()?;
        let block_end = checked_end(payload, length, end)?;
        if matches!(&key, b"Lr16" | b"Lr32" | b"Layr") && length >= 2 {
            return read_layer_info(input, version, block_end, transparency, measure_pixels)
                .map(Some);
        }
        if matches!(&key, b"Mt16" | b"Mt32" | b"Mtrn") && length == 0 {
            transparency = true;
        }
        let padding = (4 - length % 4) % 4;
        let next = checked_end(block_end, padding, end).unwrap_or(block_end);
        input.seek(SeekFrom::Start(next))?;
    }
    Ok(transparency.then_some(LayerInfo {
        alpha_marker: Some(-1),
        count: None,
        pixels: None,
    }))
}

fn read_layer_info(
    input: &mut (impl Read + Seek),
    version: u16,
    end: u64,
    transparency: bool,
    measure_pixels: bool,
) -> io::Result<LayerInfo> {
    let signed_count = i16::from_be_bytes(read(input)?);
    let count = signed_count.unsigned_abs();
    let pixels = if measure_pixels {
        count_layer_pixels(input, version, end, count).ok()
    } else {
        None
    };
    Ok(LayerInfo {
        alpha_marker: Some(if transparency { -1 } else { signed_count }),
        count: Some(count),
        pixels,
    })
}

fn count_layer_pixels(
    input: &mut (impl Read + Seek),
    version: u16,
    end: u64,
    count: u16,
) -> io::Result<u64> {
    let mut pixels = 0_u64;
    for _ in 0..count {
        checked_end(input.stream_position()?, 18, end)?;
        let top = i32::from_be_bytes(read(input)?);
        let left = i32::from_be_bytes(read(input)?);
        let bottom = i32::from_be_bytes(read(input)?);
        let right = i32::from_be_bytes(read(input)?);
        if bottom < top || right < left {
            return Err(invalid("invalid layer bounds"));
        }
        let width = (i64::from(right) - i64::from(left)) as u64;
        let height = (i64::from(bottom) - i64::from(top)) as u64;
        pixels = pixels.saturating_add(width * height);

        let channels = u64::from(u16::from_be_bytes(read(input)?));
        // Each channel is ID + size, followed by blend mode and four flag bytes.
        let extra_offset = checked_end(
            input.stream_position()?,
            channels * (2 + u64::from(version) * 4) + 12,
            end,
        )?;
        checked_end(extra_offset, 4, end)?;
        input.seek(SeekFrom::Start(extra_offset))?;
        let extra_length = u64::from(u32::from_be_bytes(read(input)?));
        let next = checked_end(input.stream_position()?, extra_length, end)?;
        input.seek(SeekFrom::Start(next))?;
    }
    Ok(pixels)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn checked_end(start: u64, length: u64, end: u64) -> io::Result<u64> {
    start
        .checked_add(length)
        .filter(|&offset| offset <= end)
        .ok_or_else(|| invalid("section exceeds file bounds"))
}

fn read<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_layer_length(input: &mut impl Read, version: u16) -> io::Result<u64> {
    if version == 2 {
        Ok(u64::from_be_bytes(read(input)?))
    } else {
        Ok(u64::from(u32::from_be_bytes(read(input)?)))
    }
}

fn write_layer_length(output: &mut impl Write, length: u32, version: u16) -> io::Result<()> {
    if version == 2 {
        output.write_all(&u64::from(length).to_be_bytes())
    } else {
        output.write_all(&length.to_be_bytes())
    }
}

impl Info {
    pub(crate) fn write_zip_proxy(
        &self,
        input: &Path,
        output: &Path,
        max_bytes: usize,
    ) -> Result<(), String> {
        self.write_proxy(input, output, max_bytes)
            .map_err(|error| format!("PSD/PSB ZIP 合成图读取失败：{error}"))
    }

    fn write_proxy(&self, input: &Path, output: &Path, max_bytes: usize) -> io::Result<()> {
        if !self.has_real_merged
            || !matches!(self.compression, 2 | 3)
            || (self.depth == 1 && self.compression == 3)
        {
            return Err(invalid("unsupported saved composite"));
        }
        let row_bytes = (u64::from(self.width) * u64::from(self.depth)).div_ceil(8);
        let rows = u64::from(self.height) * u64::from(self.channels);
        if row_bytes * rows > max_bytes as u64 {
            return Err(invalid("composite exceeds decoded byte budget"));
        }

        let mut source = BufReader::new(File::open(input)?);
        let mut destination = BufWriter::new(File::create_new(output)?);
        if io::copy(
            &mut (&mut source).take(self.layers_offset),
            &mut destination,
        )? != self.layers_offset
        {
            return Err(invalid("truncated Photoshop header or resources"));
        }
        if let Some(count) = self.alpha_marker {
            // Scene zero reads only this signed count and seeks past the section.
            // Keep the original alpha convention without any actual layer records.
            write_layer_length(
                &mut destination,
                u32::from(self.version) * 4 + 6,
                self.version,
            )?;
            write_layer_length(&mut destination, 2, self.version)?;
            destination.write_all(&(if count < 0 { -1_i16 } else { 1 }).to_be_bytes())?;
            destination.write_all(&0_u32.to_be_bytes())?;
        } else {
            write_layer_length(&mut destination, 0, self.version)?;
        }
        destination.write_all(&0_u16.to_be_bytes())?;
        source.seek(SeekFrom::Start(self.composite_offset + 2))?;
        let mut zip = Zip {
            input: source,
            decoder: Decompress::new(true),
            ended: false,
        };
        let mut row = vec![0; row_bytes as usize];
        let mut interleaved = vec![
            0;
            if self.compression == 3 && self.depth == 32 {
                row.len()
            } else {
                0
            }
        ];
        for _ in 0..rows {
            zip.read_exact(&mut row)?;
            if self.compression == 3 {
                undo_prediction(&mut row, self.depth, &mut interleaved);
            }
            destination.write_all(&row)?;
        }
        if zip.read(&mut [0])? != 0 {
            return Err(invalid("oversized ZIP composite"));
        }
        destination.flush()
    }
}

fn undo_prediction(row: &mut [u8], depth: u16, scratch: &mut [u8]) {
    if depth == 16 {
        let mut previous = 0_u16;
        for sample in row.chunks_exact_mut(2) {
            previous = previous.wrapping_add(u16::from_be_bytes([sample[0], sample[1]]));
            sample.copy_from_slice(&previous.to_be_bytes());
        }
    } else {
        let mut previous = 0_u8;
        for value in &mut *row {
            previous = previous.wrapping_add(*value);
            *value = previous;
        }
        if depth == 32 {
            let width = row.len() / 4;
            for (x, sample) in scratch.chunks_exact_mut(4).enumerate() {
                for (channel, byte) in sample.iter_mut().enumerate() {
                    *byte = row[channel * width + x];
                }
            }
            row.copy_from_slice(scratch);
        }
    }
}

struct Zip<R> {
    input: R,
    decoder: Decompress,
    ended: bool,
}

impl<R: BufRead> Read for Zip<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.ended || output.is_empty() {
            return Ok(0);
        }
        loop {
            let data = self.input.fill_buf()?;
            let empty = data.is_empty();
            let (input_start, output_start) = (self.decoder.total_in(), self.decoder.total_out());
            let flush = if empty {
                FlushDecompress::Finish
            } else {
                FlushDecompress::None
            };
            let status = self
                .decoder
                .decompress(data, output, flush)
                .map_err(io::Error::other)?;
            let consumed = (self.decoder.total_in() - input_start) as usize;
            let produced = (self.decoder.total_out() - output_start) as usize;
            self.input.consume(consumed);
            self.ended = status == Status::StreamEnd;
            if produced != 0 || self.ended {
                return Ok(produced);
            }
            if consumed == 0 {
                return Err(invalid("truncated ZIP stream or missing checksum"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::ZlibEncoder, Compression};

    fn resource(id: u16, value: &[u8], output: &mut Vec<u8>) {
        output.extend(b"8BIM");
        output.extend(id.to_be_bytes());
        output.extend([0, 0]);
        output.extend((value.len() as u32).to_be_bytes());
        output.extend(value);
        if value.len() % 2 != 0 {
            output.push(0);
        }
    }

    fn document(version: u16, depth: u16, has_real_merged: bool, data: &[u8]) -> Vec<u8> {
        let mut resources = Vec::new();
        resource(
            1057,
            &[0, 0, 0, 1, u8::from(has_real_merged)],
            &mut resources,
        );
        resource(1039, b"icc data", &mut resources);
        let mut thumbnail = Vec::new();
        for field in [1_u32, 2, 1, 8, 8, 3] {
            thumbnail.extend(field.to_be_bytes());
        }
        thumbnail.extend([0, 24, 0, 1, 0xff, 0xd8, 0xff]);
        resource(1036, &thumbnail, &mut resources);
        let mut header = b"8BPS".to_vec();
        header.extend(version.to_be_bytes());
        header.extend([0; 6]);
        header.extend(4_u16.to_be_bytes());
        header.extend(1_u32.to_be_bytes());
        header.extend(2_u32.to_be_bytes());
        header.extend(depth.to_be_bytes());
        header.extend(3_u16.to_be_bytes());
        header.extend(0_u32.to_be_bytes());
        header.extend((resources.len() as u32).to_be_bytes());
        header.extend(resources);
        write_layer_length(&mut header, u32::from(version) * 4 + 1024, version).unwrap();
        write_layer_length(&mut header, 1024, version).unwrap();
        header.extend((-2_i16).to_be_bytes());
        header.extend([0x35; 1022]);
        header.extend(3_u16.to_be_bytes());
        header.extend(data);
        header
    }

    fn compressed(plain: &[u8]) -> Vec<u8> {
        let mut zlib = ZlibEncoder::new(Vec::new(), Compression::fast());
        zlib.write_all(plain).unwrap();
        zlib.finish().unwrap()
    }

    #[test]
    fn restores_zip_prediction_and_removes_layer_payload() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        for (version, depth, input, expected) in [
            (1, 8, vec![7, 253], vec![7, 4]),
            (2, 16, vec![0x12, 0xff, 0xff, 2], vec![0x12, 0xff, 0x12, 1]),
            (
                2,
                32,
                vec![0, 4, 7, 9, 1, 2, 3, 4],
                vec![0, 11, 21, 26, 4, 20, 23, 30],
            ),
        ] {
            let destination = temp.path().join(format!("proxy-{depth}"));
            std::fs::write(
                &source,
                document(version, depth, true, &compressed(&input.repeat(4))),
            )
            .unwrap();
            let info = inspect(&source).unwrap();
            assert!(info.has_real_merged);
            assert_eq!(info.thumbnail.as_deref(), Some(&[0xff, 0xd8, 0xff][..]));
            assert_eq!(info.icc.as_deref(), Some(b"icc data".as_slice()));
            info.write_zip_proxy(&source, &destination, 32).unwrap();
            let proxy = inspect(&destination).unwrap();
            assert_eq!(proxy.compression, 0);
            assert_eq!(proxy.version, version);
            assert_eq!(proxy.alpha_marker, Some(-1));
            assert!(info.composite_offset - proxy.composite_offset > 1000);
            let contents = std::fs::read(destination).unwrap();
            assert_eq!(
                contents[(proxy.composite_offset + 2) as usize..],
                expected.repeat(4)
            );
        }
    }

    #[test]
    fn false_merged_flag_and_bad_zip_cannot_produce_a_proxy() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let mut truncated = compressed(&[100; 8]);
        truncated.pop(); // The samples alone are insufficient without the zlib trailer.
        for (index, merged, data) in [
            (0, false, compressed(&[100; 8])),
            (1, true, truncated),
            (2, true, compressed(&[100; 9])),
        ] {
            std::fs::write(&source, document(1, 8, merged, &data)).unwrap();
            let info = inspect(&source).unwrap();
            let destination = temp.path().join(format!("proxy-{index}"));
            assert_eq!(info.has_real_merged, merged);
            assert!(info.write_zip_proxy(&source, &destination, 32).is_err());
        }
    }

    #[test]
    fn counts_layer_rectangles_without_decoding_their_pixels() {
        for version in [1, 2] {
            let mut records = Vec::new();
            records.extend((-2_i16).to_be_bytes());
            for rectangle in [[-2_i32, -4, 13_000, 14_000], [0, 5, 10, 25]] {
                for coordinate in rectangle {
                    records.extend(coordinate.to_be_bytes());
                }
                records.extend(1_u16.to_be_bytes());
                records.extend(0_i16.to_be_bytes());
                write_layer_length(&mut records, 128, version).unwrap();
                records.extend(b"8BIMnorm");
                records.extend([255, 0, 0, 0]);
                records.extend(4_u32.to_be_bytes());
                records.extend([0; 4]);
            }
            let records_end = records.len() as u64;
            records.extend([0x80; 256]);
            let mut file = io::Cursor::new(records);
            let layers = read_layer_info(&mut file, version, records_end, false, true).unwrap();
            assert_eq!(layers.count, Some(2));
            assert_eq!(layers.alpha_marker, Some(-2));
            assert_eq!(layers.pixels, Some(13_002 * 14_004 + 200));
            assert_eq!(file.position(), records_end);
        }
    }
}
