use image::metadata::Orientation;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
    time::SystemTime,
};
use tiff::{
    decoder::{ChunkType, Decoder, DecodingResult, Limits},
    tags::{IfdPointer, Tag},
    ColorType, Directory,
};

const MAX_LEVELS: usize = 32;
const MAX_TILE_SIDE: u32 = 2048;
const MAX_OVERVIEW_SIDE: u32 = 2048;
const MAX_PROFILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TILES_PER_LEVEL: u32 = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Level {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) tile_width: u32,
    pub(crate) tile_height: u32,
}

pub(crate) struct Overview {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

pub(crate) struct Source {
    pub(crate) levels: Vec<Level>,
    pub(crate) orientation: Orientation,
    file: File,
    header: Header,
    offsets: Vec<IfdPointer>,
    active: ActiveLevel,
    base_profile: Option<Vec<u8>>,
    color_managed: bool,
}

struct ActiveLevel {
    index: usize,
    decoder: TiffDecoder,
    color: ColorType,
    associated_alpha: bool,
    profile: Option<Vec<u8>>,
}

impl Source {
    pub(crate) fn open(path: &Path) -> Result<Option<Self>, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = [0; 16];
        if let Err(error) = file.read_exact(&mut bytes) {
            return if error.kind() == io::ErrorKind::UnexpectedEof {
                Ok(None)
            } else {
                Err(error.to_string())
            };
        }
        let Some((header, first)) = Header::parse(bytes) else {
            return Ok(None);
        };
        let mut decoder = match header.decoder(&file, first) {
            // The ordinary decoder owns the original error for non-pyramid inputs.
            Ok(decoder) if decoder.get_chunk_type() == ChunkType::Tile => decoder,
            _ => return Ok(None),
        };
        let directory = decoder.read_directory(first).map_err(decode_error)?;
        if directory.next().is_none() && directory.get(Tag::SubIfd).is_none() {
            return Ok(None);
        }
        let (width, height) = decoder.dimensions().map_err(decode_error)?;
        let mut offsets = Vec::new();
        if let Some(sub_ifds) = directory.get(Tag::SubIfd) {
            if sub_ifds.count() > MAX_LEVELS as u64 {
                return Err("TIFF 金字塔缩放层过多".into());
            }
            let pointers = sub_ifd_offsets(decoder.get_tag(Tag::SubIfd).map_err(decode_error)?)?;
            for pointer in pointers {
                if pointer == first || offsets.iter().any(|&(_, _, offset)| offset == pointer) {
                    return Err("TIFF 金字塔目录重复".into());
                }
                let sub = decoder.read_directory(pointer).map_err(decode_error)?;
                let mut tags = decoder.read_directory_tags(&sub);
                let sub_width = tags
                    .get_tag_unsigned(Tag::ImageWidth)
                    .map_err(decode_error)?;
                let sub_height = tags
                    .get_tag_unsigned(Tag::ImageLength)
                    .map_err(decode_error)?;
                if same_image_scale(width, height, sub_width, sub_height) {
                    offsets.push((sub_width, sub_height, pointer));
                }
            }
        }
        let mut next = if offsets.is_empty() {
            directory.next()
        } else {
            None
        };
        while let Some(pointer) = next {
            if pointer == first || offsets.iter().any(|&(_, _, offset)| offset == pointer) {
                return Err("TIFF 金字塔目录重复".into());
            }
            let sub = decoder.read_directory(pointer).map_err(decode_error)?;
            let mut tags = decoder.read_directory_tags(&sub);
            let flags = tags
                .find_tag_unsigned::<u32>(Tag::NewSubfileType)
                .map_err(decode_error)?
                .unwrap_or_default();
            if flags & 7 != 1 {
                break;
            }
            let sub_width = tags
                .get_tag_unsigned(Tag::ImageWidth)
                .map_err(decode_error)?;
            let sub_height = tags
                .get_tag_unsigned(Tag::ImageLength)
                .map_err(decode_error)?;
            if !same_image_scale(width, height, sub_width, sub_height) {
                break;
            }
            if offsets.len() >= MAX_LEVELS - 1 {
                return Err("TIFF 金字塔缩放层过多".into());
            }
            offsets.push((sub_width, sub_height, pointer));
            next = sub.next();
        }
        if offsets.is_empty() {
            return Ok(None);
        }
        if offsets.len() >= MAX_LEVELS {
            return Err("TIFF 金字塔缩放层过多".into());
        }
        offsets.sort_unstable_by_key(|&(w, h, _)| std::cmp::Reverse((w, h)));
        offsets.dedup_by_key(|(w, h, _)| (*w, *h));
        let orientation = orientation(&mut decoder)?.unwrap_or(Orientation::NoTransforms);
        let mut levels = vec![level(&mut decoder, false)?];
        let mut active = ActiveLevel::new(0, decoder, &directory)?;
        let base_profile = active.profile.take();
        let mut offsets: Vec<IfdPointer> =
            offsets.into_iter().map(|(_, _, pointer)| pointer).collect();
        offsets.insert(0, first);
        for &pointer in &offsets[1..] {
            let mut decoder = header.decoder(&file, pointer).map_err(decode_error)?;
            if self::orientation(&mut decoder)?
                .is_some_and(|value| value != orientation && value != Orientation::NoTransforms)
            {
                return Err("TIFF 金字塔各层方向不一致".into());
            }
            let metadata = level(&mut decoder, true)?;
            supported_color(&mut decoder)?;
            levels.push(metadata);
        }
        if !levels
            .iter()
            .skip(1)
            .any(|level| level.width.max(level.height) <= MAX_OVERVIEW_SIDE)
        {
            return Err("TIFF 金字塔缺少可快速打开的小尺寸全景层".into());
        }
        Ok(Some(Self {
            file,
            header,
            levels,
            offsets,
            orientation,
            active,
            base_profile,
            color_managed: true,
        }))
    }

    pub(crate) fn set_color_managed(&mut self, enabled: bool) {
        self.color_managed = enabled;
    }

    pub(crate) fn stamp(&self) -> Result<(u64, Option<SystemTime>), String> {
        let metadata = self.file.metadata().map_err(|error| error.to_string())?;
        Ok((metadata.len(), metadata.modified().ok()))
    }

    pub(crate) fn overview(&mut self) -> Result<Overview, String> {
        let index = self
            .levels
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, level)| {
                (level.width.max(level.height) <= MAX_OVERVIEW_SIDE).then_some(index)
            })
            .ok_or("TIFF 金字塔缺少小尺寸全景层")?;
        let Level {
            width,
            height,
            tile_width,
            tile_height,
        } = self.levels[index];
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for y in 0..height.div_ceil(tile_height) {
            for x in 0..width.div_ceil(tile_width) {
                let left = x * tile_width;
                let top = y * tile_height;
                let tile = self.read_tile(index, x, y)?;
                let row_bytes = (width - left).min(tile_width) as usize * 4;
                let offset = (top as usize * width as usize + left as usize) * 4;
                for (row, src) in tile.chunks_exact(row_bytes).enumerate() {
                    let dest = offset + row * width as usize * 4;
                    rgba[dest..dest + row_bytes].copy_from_slice(src);
                }
            }
        }
        Ok(Overview {
            width,
            height,
            rgba,
        })
    }

    pub(crate) fn read_tile(&mut self, level: usize, x: u32, y: u32) -> Result<Vec<u8>, String> {
        let Level {
            width,
            height,
            tile_width,
            tile_height,
        } = *self.levels.get(level).ok_or("TIFF 缩放层无效")?;
        let columns = width.div_ceil(tile_width);
        if x >= columns || y >= height.div_ceil(tile_height) {
            return Err("TIFF 分块坐标无效".into());
        }
        if self.active.index != level {
            let offset = self.offsets[level];
            let mut decoder = self
                .header
                .decoder(&self.file, offset)
                .map_err(decode_error)?;
            let directory = decoder.read_directory(offset).map_err(decode_error)?;
            self.active = ActiveLevel::new(level, decoder, &directory)?;
        }
        let index = y * columns + x;
        let width = (width - x * tile_width).min(tile_width);
        let height = (height - y * tile_height).min(tile_height);
        let samples = self
            .active
            .decoder
            .read_chunk(index)
            .map_err(decode_error)?;
        let mut rgba = rgba8(samples, self.active.color, width as usize * height as usize)?;
        if self.active.associated_alpha {
            for pixel in rgba.chunks_exact_mut(4) {
                let alpha = u32::from(pixel[3]);
                for value in &mut pixel[..3] {
                    *value = if alpha == 0 {
                        0
                    } else {
                        ((u32::from(*value) * 255 + alpha / 2) / alpha).min(255) as u8
                    };
                }
            }
        }
        if self.color_managed {
            if let Some(profile) = self
                .active
                .profile
                .as_deref()
                .or(self.base_profile.as_deref())
            {
                let _ = crate::imaging::apply_icc(profile, &mut rgba);
            }
        }
        Ok(rgba)
    }
}

impl ActiveLevel {
    fn new(index: usize, mut decoder: TiffDecoder, directory: &Directory) -> Result<Self, String> {
        let color = supported_color(&mut decoder)?;
        let associated_alpha = decoder
            .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
            .map_err(decode_error)?
            .is_some_and(|values| values.first() == Some(&1));
        let profile = directory
            .get(Tag::IccProfile)
            .filter(|entry| entry.count() <= MAX_PROFILE_BYTES)
            .and_then(|_| decoder.get_tag(Tag::IccProfile).ok())
            .and_then(|value| value.into_u8_vec().ok());
        Ok(Self {
            index,
            decoder,
            color,
            associated_alpha,
            profile,
        })
    }
}

fn same_image_scale(width: u32, height: u32, reduced_width: u32, reduced_height: u32) -> bool {
    // Pyramid generators differ in whether odd dimensions are rounded up or down.
    let cross = u64::from(reduced_width) * u64::from(height);
    let other = u64::from(reduced_height) * u64::from(width);
    reduced_width > 0
        && reduced_height > 0
        && reduced_width <= width
        && reduced_height <= height
        && (reduced_width < width || reduced_height < height)
        && cross.abs_diff(other) <= u64::from(width) + u64::from(height)
}

fn sub_ifd_offsets(value: tiff::decoder::ifd::Value) -> Result<Vec<IfdPointer>, String> {
    use tiff::decoder::ifd::Value;
    let values = match value {
        Value::List(values) => values,
        value => vec![value],
    };
    values
        .into_iter()
        .map(|value| match value {
            // BigTIFF producers can use LONG8 as well as the explicit IFD8 type.
            Value::UnsignedBig(offset) => Ok(IfdPointer(offset)),
            value => value.into_ifd_pointer().map_err(decode_error),
        })
        .collect()
}

fn level(decoder: &mut TiffDecoder, overview: bool) -> Result<Level, String> {
    let (width, height) = decoder.dimensions().map_err(decode_error)?;
    let (tile_width, tile_height) = decoder.chunk_dimensions();
    let tiles = decoder.get_chunk_type() == ChunkType::Tile;
    let tile_height = if tiles {
        tile_height
    } else {
        tile_height.min(height)
    };
    if width == 0
        || height == 0
        || tile_width == 0
        || tile_height == 0
        || tile_width > MAX_TILE_SIDE
        || tile_height > MAX_TILE_SIDE
        || (tiles && (tile_width < 16 || tile_height < 16))
        || (!tiles && (!overview || width.max(height) > MAX_OVERVIEW_SIDE))
        || width
            .div_ceil(tile_width)
            .saturating_mul(height.div_ceil(tile_height))
            > MAX_TILES_PER_LEVEL
    {
        return Err("TIFF 金字塔分块尺寸或数量超出限额".into());
    }
    Ok(Level {
        width,
        height,
        tile_width,
        tile_height,
    })
}

fn supported_color(decoder: &mut TiffDecoder) -> Result<ColorType, String> {
    let color = decoder.colortype().map_err(decode_error)?;
    let chunky = decoder
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .map_err(decode_error)?
        .unwrap_or(1)
        == 1;
    let unsigned = decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(decode_error)?
        .is_none_or(|values| values.iter().all(|&value| value == 1));
    let compression = decoder
        .find_tag_unsigned::<u16>(Tag::Compression)
        .map_err(decode_error)?
        .unwrap_or(1);
    if !matches!(compression, 1 | 5 | 7 | 8 | 0x80B2 | 0x8005) {
        return Err(format!("TIFF 金字塔压缩方式暂不支持：{compression}"));
    }
    if !chunky
        || !unsigned
        || !matches!(
            color,
            ColorType::Gray(8 | 16)
                | ColorType::GrayA(8 | 16)
                | ColorType::RGB(8 | 16)
                | ColorType::RGBA(8 | 16)
                | ColorType::Multiband {
                    bit_depth: 8 | 16,
                    num_samples: 2
                }
        ) && !(compression == 7 && color == ColorType::YCbCr(8))
    {
        return Err(format!("TIFF 金字塔样本格式暂不支持：{color:?}"));
    }
    Ok(color)
}

fn rgba8(samples: DecodingResult, color: ColorType, pixels: usize) -> Result<Vec<u8>, String> {
    let channels = match color {
        ColorType::Gray(_) => 1,
        ColorType::GrayA(_) | ColorType::Multiband { num_samples: 2, .. } => 2,
        ColorType::RGB(_) | ColorType::YCbCr(_) => 3,
        ColorType::RGBA(_) => 4,
        _ => return Err("TIFF 金字塔样本格式无效".into()),
    };
    let mut rgba = Vec::with_capacity(pixels * 4);
    let mut push = |pixel: &[u8]| match channels {
        1 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
        2 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
        3 if color == ColorType::YCbCr(8) => {
            let y = i32::from(pixel[0]) * 65536 + 32768;
            let cb = i32::from(pixel[1]) - 128;
            let cr = i32::from(pixel[2]) - 128;
            let clip = |value: i32| (value >> 16).clamp(0, 255) as u8;
            rgba.extend_from_slice(&[
                clip(y + 91881 * cr),
                clip(y - 22554 * cb - 46802 * cr),
                clip(y + 116130 * cb),
                255,
            ]);
        }
        3 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
        4 => rgba.extend_from_slice(pixel),
        _ => unreachable!(),
    };
    match samples {
        DecodingResult::U8(samples) if channels == 4 && samples.len() == pixels * 4 => {
            return Ok(samples)
        }
        DecodingResult::U8(samples) if samples.len() == pixels * channels => {
            for pixel in samples.chunks_exact(channels) {
                push(pixel);
            }
        }
        DecodingResult::U16(samples) if samples.len() == pixels * channels => {
            for pixel in samples.chunks_exact(channels) {
                let mut converted = [0; 4];
                for (value, output) in pixel.iter().zip(&mut converted) {
                    *output = ((u32::from(*value) + 128) / 257) as u8;
                }
                push(&converted[..channels]);
            }
        }
        _ => return Err("TIFF 金字塔分块样本格式或数量无效".into()),
    }
    Ok(rgba)
}

fn orientation(decoder: &mut TiffDecoder) -> Result<Option<Orientation>, String> {
    decoder
        .find_tag_unsigned::<u8>(Tag::Orientation)
        .map_err(decode_error)?
        .map(|value| Orientation::from_exif(value).ok_or_else(|| "TIFF 金字塔方向标记无效".into()))
        .transpose()
}

fn decode_error(error: tiff::TiffError) -> String {
    format!("TIFF 金字塔读取失败：{error}")
}

#[derive(Clone, Copy)]
struct Header {
    bytes: [u8; 16],
    big: bool,
    little: bool,
}

impl Header {
    fn parse(bytes: [u8; 16]) -> Option<(Self, IfdPointer)> {
        let (big, little) = match &bytes[..4] {
            b"II\x2a\0" => (false, true),
            b"MM\0\x2a" => (false, false),
            b"II\x2b\0" if bytes[4..8] == [8, 0, 0, 0] => (true, true),
            b"MM\0\x2b" if bytes[4..8] == [0, 8, 0, 0] => (true, false),
            _ => return None,
        };
        let offset = match (big, little) {
            (false, true) => u64::from(u32::from_le_bytes(bytes[4..8].try_into().unwrap())),
            (false, false) => u64::from(u32::from_be_bytes(bytes[4..8].try_into().unwrap())),
            (true, true) => u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            (true, false) => u64::from_be_bytes(bytes[8..16].try_into().unwrap()),
        };
        Some((Self { bytes, big, little }, IfdPointer(offset)))
    }

    fn decoder(self, file: &File, offset: IfdPointer) -> tiff::TiffResult<TiffDecoder> {
        let mut bytes = self.bytes;
        let header_len = if self.big {
            bytes[8..16].copy_from_slice(&if self.little {
                offset.0.to_le_bytes()
            } else {
                offset.0.to_be_bytes()
            });
            16
        } else {
            let offset = u32::try_from(offset.0)?;
            bytes[4..8].copy_from_slice(&if self.little {
                offset.to_le_bytes()
            } else {
                offset.to_be_bytes()
            });
            8
        };
        // The pinned decoder only follows the main IFD chain. Present the selected IFD as the
        // first one in a virtual header, with all actual pixel offsets and source bytes intact.
        let decoder = Decoder::new(BufReader::new(LevelStream {
            file: file.try_clone()?,
            bytes,
            header_len,
            position: 0,
        }))?;
        let mut limits = Limits::default();
        limits.decoding_buffer_size = 40 * 1024 * 1024;
        limits.intermediate_buffer_size = 40 * 1024 * 1024;
        Ok(decoder.with_limits(limits))
    }
}

type TiffDecoder = Decoder<BufReader<LevelStream>>;

struct LevelStream {
    file: File,
    bytes: [u8; 16],
    header_len: usize,
    position: u64,
}

impl Read for LevelStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // File clones share a cursor. Each decoder retains its own logical position.
        self.file.seek(SeekFrom::Start(self.position))?;
        let count = self.file.read(buffer)?;
        if self.position < self.header_len as u64 {
            let start = self.position as usize;
            let len = count.min(self.header_len - start);
            buffer[..len].copy_from_slice(&self.bytes[start..start + len]);
        }
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for LevelStream {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.position = match position {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.file.metadata()?.len().checked_add_signed(offset),
        }
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "TIFF offset outside file position range",
            )
        })?;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::encoder::{DirectoryEncoder, TiffEncoder, TiffKind, TiffKindBig, TiffKindStandard};

    fn write_tiles<K: TiffKind>(
        directory: &mut DirectoryEncoder<'_, File, K>,
        width: u32,
        height: u32,
    ) {
        let mut offsets = Vec::new();
        let mut counts = Vec::new();
        for index in 0..width.div_ceil(16) * height.div_ceil(16) {
            let pixels = [90 + index as u8, 40, 160].repeat(16 * 16);
            offsets.push(K::convert_offset(directory.write_data(&pixels[..]).unwrap()).unwrap());
            counts.push(K::convert_offset(pixels.len() as u64).unwrap());
        }
        directory.write_tag(Tag::ImageWidth, width).unwrap();
        directory.write_tag(Tag::ImageLength, height).unwrap();
        directory
            .write_tag(Tag::PhotometricInterpretation, 2_u16)
            .unwrap();
        directory.write_tag(Tag::SamplesPerPixel, 3_u16).unwrap();
        directory
            .write_tag(Tag::BitsPerSample, &[8_u16; 3][..])
            .unwrap();
        directory.write_tag(Tag::TileWidth, 16_u32).unwrap();
        directory.write_tag(Tag::TileLength, 16_u32).unwrap();
        directory
            .write_tag(Tag::TileOffsets, K::convert_slice(&offsets))
            .unwrap();
        directory
            .write_tag(Tag::TileByteCounts, K::convert_slice(&counts))
            .unwrap();
    }

    fn write_pyramid<K: TiffKind>(file: File, sub_ifd: bool, reduced: bool) {
        let mut encoder = TiffEncoder::<_, K>::new_generic(file).unwrap();
        let sub_ifd_pointer = sub_ifd.then(|| {
            let mut overview = encoder.extra_directory().unwrap();
            write_tiles(&mut overview, 17, 10);
            overview.finish_with_offsets().unwrap().pointer
        });
        let mut base = encoder.image_directory().unwrap();
        write_tiles(&mut base, 35, 21);
        base.write_tag(Tag::Orientation, 6_u16).unwrap();
        if let Some(pointer) = sub_ifd_pointer {
            base.write_tag(Tag::SubIfd, K::convert_offset(pointer.0).unwrap())
                .unwrap();
        }
        base.finish().unwrap();
        if !sub_ifd {
            let mut overview = encoder.image_directory().unwrap();
            write_tiles(&mut overview, 17, 10);
            if reduced {
                overview.write_tag(Tag::NewSubfileType, 1_u32).unwrap();
            }
            overview.finish().unwrap();
        }
    }

    #[test]
    fn reads_pyramid_tiles_and_overview_in_classic_and_big_tiff() {
        let temp = tempfile::tempdir().unwrap();
        for sub_ifd in [false, true] {
            for big in [false, true] {
                let path = temp.path().join(format!("tiles-{sub_ifd}-{big}.tiff"));
                let file = File::create(&path).unwrap();
                if big {
                    write_pyramid::<TiffKindBig>(file, sub_ifd, true);
                } else {
                    write_pyramid::<TiffKindStandard>(file, sub_ifd, true);
                }
                let mut source = Source::open(&path).unwrap().unwrap();
                assert_eq!(source.orientation, Orientation::Rotate90);
                assert_eq!(source.levels.len(), 2);
                assert_eq!((source.levels[0].width, source.levels[0].height), (35, 21));
                let edge = source.read_tile(0, 2, 1).unwrap();
                assert_eq!(edge, [95, 40, 160, 255].repeat(3 * 5));
                assert!(source.read_tile(0, 3, 0).is_err());
                let overview = source.overview().unwrap();
                assert_eq!((overview.width, overview.height), (17, 10));
                for row in overview.rgba.chunks_exact(17 * 4) {
                    assert_eq!(row[..16 * 4], [90, 40, 160, 255].repeat(16));
                    assert_eq!(row[16 * 4..], [91, 40, 160, 255]);
                }
                assert_eq!(source.read_tile(0, 2, 1).unwrap(), edge);
            }
        }
    }

    #[test]
    fn independent_tiff_pages_remain_ordinary_images() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("pages.tiff");
        write_pyramid::<TiffKindStandard>(File::create(&path).unwrap(), false, false);
        assert!(Source::open(&path).unwrap().is_none());
    }
}
