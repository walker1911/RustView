use image::{metadata::Orientation, DynamicImage, RgbaImage};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};
use tiff::{
    decoder::{Decoder, DecodingResult, Limits},
    tags::Tag,
    ColorType,
};

pub struct Page {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub index: u32,
    pub total: u32,
}

fn push_pixel(output: &mut Vec<u8>, samples: &[u8], channels: usize) {
    match channels {
        1 => output.extend_from_slice(&[samples[0], samples[0], samples[0], 255]),
        2 => output.extend_from_slice(&[samples[0], samples[0], samples[0], samples[1]]),
        3 => output.extend_from_slice(&[samples[0], samples[1], samples[2], 255]),
        4 => output.extend_from_slice(&samples[..4]),
        _ => unreachable!(),
    }
}

/// Visits image-directory headers only. Pixel data is decoded for the requested page alone.
pub fn decode(path: &Path, page: u32, last: bool, max_bytes: usize) -> Result<Page, String> {
    decode_with_options(path, page, last, max_bytes, true, false)
}

pub(crate) fn decode_with_options(
    path: &Path,
    page: u32,
    last: bool,
    max_bytes: usize,
    managed: bool,
    full_raw: bool,
) -> Result<Page, String> {
    let input_size = path.metadata().map_err(|error| error.to_string())?.len();
    if input_size > 512 * 1024 * 1024 {
        return Err("输入文件超过512MiB读取上限".into());
    }
    let mut signature = [0; 4];
    let mut input = File::open(path).map_err(|error| error.to_string())?;
    let count = input
        .read(&mut signature)
        .map_err(|error| error.to_string())?;
    let tiff = count == 4
        && matches!(
            &signature,
            b"II\x2a\0" | b"MM\0\x2a" | b"II\x2b\0" | b"MM\0\x2b"
        );
    if !tiff || crate::imaging::is_raw(&path.to_string_lossy()) {
        let decode = if managed {
            crate::imaging::decode
        } else {
            crate::imaging::decode_unmanaged
        };
        let decoded = decode(path, 200_000_000, max_bytes, full_raw)?;
        return Ok(Page {
            width: decoded.width,
            height: decoded.height,
            rgba: decoded.rgba,
            index: 0,
            total: 1,
        });
    }
    let mut limits = Limits::default();
    limits.decoding_buffer_size = max_bytes;
    limits.intermediate_buffer_size = max_bytes;
    limits.ifd_value_size = 4 * 1024 * 1024;
    let input = File::open(path).map_err(|error| error.to_string())?;
    let mut decoder = Decoder::new(BufReader::new(input))
        .map_err(|error| format!("TIFF头读取失败：{error}"))?
        .with_limits(limits);
    let mut total = 1_u32;
    while decoder.more_images() {
        if total >= 10_000 {
            return Err("TIFF超过一万页索引上限".into());
        }
        decoder
            .next_image()
            .map_err(|error| format!("TIFF页索引读取失败：{error}"))?;
        total += 1;
    }
    let index = if last { total - 1 } else { page.min(total - 1) };
    decoder
        .seek_to_image(index as usize)
        .map_err(|error| format!("无法定位TIFF第{}页：{error}", index + 1))?;
    let (width, height) = decoder.dimensions().map_err(|error| error.to_string())?;
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .filter(|pixels| *pixels > 0 && *pixels <= 200_000_000)
        .ok_or("TIFF页尺寸无效或超过像素限制")?;
    let output_bytes = pixels
        .checked_mul(4)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .filter(|bytes| *bytes <= max_bytes)
        .ok_or("TIFF页超过RGBA内存预算")?;
    let color = decoder.colortype().map_err(|error| error.to_string())?;
    let (channels, bits) = match color {
        ColorType::Gray(bits) => (1, bits),
        ColorType::GrayA(bits) => (2, bits),
        ColorType::RGB(bits) => (3, bits),
        ColorType::RGBA(bits) => (4, bits),
        other => return Err(format!("暂不支持此TIFF颜色类型：{other:?}")),
    };
    if !matches!(bits, 8 | 16) {
        return Err(format!(
            "暂不支持{bits}位TIFF样本；当前支持8/16位RGB、RGBA和灰度"
        ));
    }
    let orientation = decoder
        .find_tag_unsigned::<u8>(Tag::Orientation)
        .map_err(|error| format!("TIFF方向标记无效：{error}"))?
        .unwrap_or(1);
    let orientation = Orientation::from_exif(orientation).ok_or("TIFF方向标记无效")?;
    let profile = if managed {
        decoder
            .find_tag(Tag::IccProfile)
            .map_err(|error| error.to_string())?
            .map(|value| value.into_u8_vec())
            .transpose()
            .map_err(|error| error.to_string())?
    } else {
        None
    };
    let associated = decoder
        .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
        .map_err(|error| error.to_string())?
        .is_some_and(|values| values.first() == Some(&1));
    let samples = decoder
        .read_image()
        .map_err(|error| format!("TIFF第{}页解码失败：{error}", index + 1))?;
    let expected = (pixels as usize)
        .checked_mul(channels)
        .ok_or("TIFF样本数量溢出")?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(output_bytes)
        .map_err(|_| "无法在预算内分配TIFF页像素")?;
    match samples {
        DecodingResult::U8(samples) if bits == 8 && samples.len() == expected => {
            for pixel in samples.chunks_exact(channels) {
                push_pixel(&mut rgba, pixel, channels);
            }
        }
        DecodingResult::U16(samples) if bits == 16 && samples.len() == expected => {
            for pixel in samples.chunks_exact(channels) {
                let mut converted = [0_u8; 4];
                for (output, value) in converted.iter_mut().zip(pixel) {
                    *output = ((*value as u32 + 128) / 257) as u8;
                }
                push_pixel(&mut rgba, &converted, channels);
            }
        }
        _ => return Err("TIFF样本格式或数量不受支持，未将其它数据当作8位图片显示".into()),
    }
    if associated && matches!(channels, 2 | 4) {
        for pixel in rgba.chunks_exact_mut(4) {
            let alpha = pixel[3] as u32;
            for value in &mut pixel[..3] {
                *value = if alpha == 0 {
                    0
                } else {
                    ((*value as u32 * 255 + alpha / 2) / alpha).min(255) as u8
                };
            }
        }
    }
    if let Some(profile) = profile {
        crate::imaging::apply_icc(&profile, &mut rgba)
            .map_err(|error| format!("TIFF色彩配置无法转换：{error}"))?;
    }
    let image = RgbaImage::from_raw(width, height, rgba).ok_or("TIFF页长度不一致")?;
    let mut image = DynamicImage::ImageRgba8(image);
    image.apply_orientation(orientation);
    let image = image.into_rgba8();
    Ok(Page {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        index,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::encoder::{colortype, TiffEncoder};

    #[test]
    fn selects_two_pages_last_page_and_orientation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pages.tiff");
        let file = File::create(&path).unwrap();
        let mut encoder = TiffEncoder::new(file).unwrap();
        let mut first = encoder.new_image::<colortype::RGB8>(2, 1).unwrap();
        first.encoder().write_tag(Tag::Orientation, 6_u16).unwrap();
        first.write_data(&[255, 0, 0, 0, 255, 0]).unwrap();
        encoder
            .write_image::<colortype::Gray16>(1, 1, &[65535])
            .unwrap();
        drop(encoder);
        let first = decode(&path, 0, false, 128).unwrap();
        assert_eq!(
            (first.width, first.height, first.index, first.total),
            (1, 2, 0, 2)
        );
        assert_eq!(first.rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
        let last = decode(&path, 0, true, 128).unwrap();
        assert_eq!(
            (last.width, last.height, last.index, last.total),
            (1, 1, 1, 2)
        );
        assert_eq!(last.rgba, &[255, 255, 255, 255]);
        assert!(decode(&path, 0, false, 7).is_err());
    }

    #[test]
    fn unsupported_tiff_samples_do_not_fake_a_page() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cmyk.tiff");
        let file = File::create(&path).unwrap();
        TiffEncoder::new(file)
            .unwrap()
            .write_image::<colortype::CMYK8>(1, 1, &[0, 0, 0, 0])
            .unwrap();
        assert!(decode(&path, 0, false, 128)
            .err()
            .unwrap()
            .contains("颜色类型"));
    }

    #[test]
    fn ordinary_image_is_one_page_not_next_file() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/image.png");
        let page = decode(&path, 20, true, 128).unwrap();
        assert_eq!(
            (page.width, page.height, page.index, page.total),
            (1, 1, 0, 1)
        );
    }
}
