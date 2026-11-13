use crate::{
    imaging::{self, DecodedImage},
    psd_composite::Info,
};
use std::{
    fs::File,
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

pub(crate) fn decode_rgb8(
    path: &Path,
    info: &Info,
    max_bytes: usize,
    managed: bool,
) -> Result<Option<DecodedImage>, String> {
    if !info.has_real_merged
        || (info.mode, info.depth, info.channels) != (3, 8, 3)
        || !matches!(info.compression, 0 | 1)
    {
        return Ok(None);
    }
    let (width, height) = (info.width, info.height);
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|&n| n <= max_bytes)
        .ok_or("PSD/PSB 合成图超过像素或RGBA内存预算")?;
    let pixels = (|| -> io::Result<Vec<u8>> {
        let mut file = BufReader::with_capacity(64 * 1024, File::open(path)?);
        file.seek(SeekFrom::Start(info.composite_offset + 2))?;
        let row_count = height as usize * 3;
        let mut sizes = Vec::new();
        if info.compression == 1 {
            let size_bytes = if info.version == 2 { 4 } else { 2 };
            let mut table = vec![0; row_count * size_bytes];
            file.read_exact(&mut table)?;
            for chunk in table.chunks_exact(size_bytes) {
                let mut size = [0; 4];
                size[4 - size_bytes..].copy_from_slice(chunk);
                let size = u32::from_be_bytes(size) as usize;
                // A literal byte can use one control byte; allow row padding too.
                if size == 0 || size > width as usize * 2 + 64 {
                    return Err(invalid_packbits());
                }
                sizes.push(size);
            }
            let data_bytes = sizes.iter().map(|&size| size as u64).sum::<u64>();
            if file.stream_position()? + data_bytes > file.get_ref().metadata()?.len() {
                return Err(invalid_packbits());
            }
        } else if file.stream_position()? + (bytes / 4 * 3) as u64
            > file.get_ref().metadata()?.len()
        {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated PSD composite",
            ));
        }
        let mut rgba = vec![255; bytes];
        let mut row = vec![0; width as usize];
        let mut encoded = Vec::new();
        let stride = width as usize * 4;
        for channel in 0..3 {
            for (y, dest) in rgba.chunks_exact_mut(stride).enumerate() {
                if sizes.is_empty() {
                    file.read_exact(&mut row)?;
                } else {
                    encoded.resize(sizes[channel * height as usize + y], 0);
                    file.read_exact(&mut encoded)?;
                    unpack_row(&encoded, &mut row)?;
                }
                for (pixel, &value) in dest.chunks_exact_mut(4).zip(&row) {
                    pixel[channel] = value;
                }
            }
        }
        Ok(rgba)
    })()
    .map_err(|error| format!("PSD/PSB 合成图读取失败：{error}"))?;
    let image =
        image::RgbaImage::from_raw(width, height, pixels).ok_or("PSD/PSB 合成图像素不完整")?;
    let mut image = image::DynamicImage::ImageRgba8(image);
    image.apply_orientation(info.orientation);
    let mut image = image.into_rgba8();
    let mut note = "PSD/PSB · 文档合成预览".to_owned();
    if let Some(profile) = info.icc.as_deref().filter(|_| managed) {
        let result = (|| {
            let path = crate::precision::engine_dir()?.join("sRGB.icc");
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if profile == bytes {
                return Ok(());
            }
            let target = moxcms::ColorProfile::new_from_slice(&bytes).map_err(|e| e.to_string())?;
            imaging::apply_icc_to(
                profile,
                image.as_mut(),
                &target,
                moxcms::TransformOptions {
                    rendering_intent: moxcms::RenderingIntent::Perceptual,
                    ..Default::default()
                },
            )
        })();
        match result {
            Ok(()) => note.push_str(" · ICC→sRGB"),
            Err(_) => return Ok(None), // Let the native decoder handle uncommon profiles.
        }
    }
    Ok(Some(DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        note,
    }))
}

pub(crate) fn thumbnail(info: &Info, managed: bool) -> Option<DecodedImage> {
    let bytes = info.thumbnail.as_deref()?;
    let image = decode_thumbnail(bytes, info.orientation, managed, 2048)?;
    Some(DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        note: format!(
            "PSD/PSB · 仅内嵌预览 · 原画布 {} × {}",
            info.width, info.height
        ),
    })
}

pub(crate) fn decode_thumbnail(
    bytes: &[u8],
    orientation: image::metadata::Orientation,
    managed: bool,
    max_side: u32,
) -> Option<image::RgbaImage> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Jpeg);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let profile = if managed {
        image::ImageDecoder::icc_profile(&mut decoder)
            .ok()
            .flatten()
    } else {
        None
    };
    let mut image = image::DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    let mut image = image.into_rgba8();
    if let Some(profile) = profile {
        let _ = imaging::apply_icc(&profile, image.as_mut());
    }
    Some(image)
}

fn invalid_packbits() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid PSD PackBits row")
}

fn unpack_row(mut encoded: &[u8], mut row: &mut [u8]) -> io::Result<()> {
    while !row.is_empty() {
        let (control, rest) = encoded.split_first().ok_or_else(invalid_packbits)?;
        encoded = rest;
        let control = *control as i8;
        match control {
            -128 => {}
            0..=127 => {
                let count = control as usize + 1;
                let (dest, remainder) = row
                    .split_at_mut_checked(count)
                    .ok_or_else(invalid_packbits)?;
                let (source, remaining) = encoded
                    .split_at_checked(count)
                    .ok_or_else(invalid_packbits)?;
                dest.copy_from_slice(source);
                row = remainder;
                encoded = remaining;
            }
            _ => {
                let count = (1 - i16::from(control)) as usize;
                let (dest, remainder) = row
                    .split_at_mut_checked(count)
                    .ok_or_else(invalid_packbits)?;
                let (value, remaining) = encoded.split_first().ok_or_else(invalid_packbits)?;
                dest.fill(*value);
                row = remainder;
                encoded = remaining;
            }
        }
    }
    if encoded.iter().any(|&byte| byte != 0 && byte != 128) {
        return Err(invalid_packbits());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_rejects_rows_that_cross_the_output_boundary() {
        let mut row = [0; 5];
        unpack_row(&[128, 1, 7, 8, 254, 9], &mut row).unwrap();
        assert_eq!(row, [7, 8, 9, 9, 9]);
        assert!(unpack_row(&[251, 1], &mut row).is_err());
        assert!(unpack_row(&[4, 1], &mut row).is_err());
    }
}
