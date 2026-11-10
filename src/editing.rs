use image::{ExtendedColorType, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

pub const MAX_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorDepth {
    Rgba8,
    BlackWhite1,
    Gray4,
    Gray8,
    Indexed4,
    Indexed8,
    Rgb15,
    Rgb24,
    Gray16,
    Rgb48,
}

impl ColorDepth {
    pub fn precision_note(self) -> &'static str {
        match self {
            Self::Gray16 | Self::Rgb48 => {
                "高位导出从源文件重新渲染；8位源不能恢复已经丢失的精度。显式色深转换合成透明到白色。"
            }
            Self::Rgba8 => "",
            _ => "显式色深转换会合成透明到白色。原文件首次覆盖前另有备份。",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Edit {
    WhiteBalance {
        red: f32,
        green: f32,
        blue: f32,
    },
    Tone {
        brightness: f32,
        contrast: f32,
        gamma: f32,
    },
    Levels {
        black: f32,
        white: f32,
        gamma: f32,
    },
    Saturation {
        percent: f32,
    },
    Hue {
        degrees: f32,
    },
    ChannelMixer {
        matrix: [f32; 9],
    },
    AutoLevels,
    AutoContrast,
    Sharpen {
        sigma: f32,
        amount: f32,
        threshold: f32,
    },
    Blur {
        sigma: f32,
    },
    Denoise {
        radius: u8,
    },
    Sepia {
        percent: f32,
    },
    Vignette {
        sigma: f32,
    },
    Negate,
    Solarize {
        threshold: f32,
    },
    Posterize {
        levels: u16,
    },
    Emboss {
        sigma: f32,
    },
    Edge {
        radius: f32,
    },
    OilPaint {
        radius: f32,
    },
    Spread {
        radius: f32,
    },
    Swirl {
        degrees: f32,
    },
    Wave {
        amplitude: f32,
        wavelength: f32,
    },
    Rotate {
        degrees: f32,
        background: [u8; 4],
    },
    LensDistortion {
        coefficients: [f32; 4],
    },
    Text {
        text: String,
        x: i32,
        y: i32,
        size: u32,
        color: [u8; 4],
    },
    Watermark {
        path: std::path::PathBuf,
        x: i32,
        y: i32,
        opacity: f32,
        width: Option<u32>,
        height: Option<u32>,
    },
    Resize {
        width: u32,
        height: u32,
    },
    Exposure {
        ev: f32,
    },
    Rotate90 {
        turns: u8,
    },
    FlipH,
    FlipV,
    Grayscale,
    ColorDepth {
        mode: ColorDepth,
    },
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

pub struct EditedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub color_depth: ColorDepth,
}

fn bytes(width: u32, height: u32, channels: usize) -> Result<usize, String> {
    if width == 0 || height == 0 {
        return Err("图片宽高必须大于零".into());
    }
    let size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(channels))
        .ok_or("图片尺寸溢出")?;
    if size > MAX_BYTES {
        return Err("单幅编辑图像超过512 MiB限额".into());
    }
    Ok(size)
}

fn validate(image: &EditedImage) -> Result<(), String> {
    if bytes(image.width, image.height, 4)? != image.rgba.len() {
        return Err("RGBA长度与图片尺寸不一致".into());
    }
    Ok(())
}

fn luma(rgb: &[u8]) -> u8 {
    ((54 * rgb[0] as u32 + 183 * rgb[1] as u32 + 19 * rgb[2] as u32 + 128) >> 8) as u8
}

fn rgb_on_white(pixel: &[u8]) -> [u8; 3] {
    let alpha = pixel[3] as u32;
    std::array::from_fn(|i| ((pixel[i] as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8)
}

const PALETTE16: [[u8; 3]; 16] = [
    [0, 0, 0],
    [0, 0, 128],
    [0, 128, 0],
    [0, 128, 128],
    [128, 0, 0],
    [128, 0, 128],
    [128, 128, 0],
    [192, 192, 192],
    [128, 128, 128],
    [0, 0, 255],
    [0, 255, 0],
    [0, 255, 255],
    [255, 0, 0],
    [255, 0, 255],
    [255, 255, 0],
    [255, 255, 255],
];

fn level(value: u8, maximum: u16) -> u8 {
    let index = (value as u16 * maximum + 127) / 255;
    ((index * 255 + maximum / 2) / maximum) as u8
}

fn palette_index(rgb: [u8; 3], four_bit: bool) -> u8 {
    if four_bit {
        PALETTE16
            .iter()
            .enumerate()
            .min_by_key(|(_, color)| {
                (0..3)
                    .map(|i| (rgb[i] as i32 - color[i] as i32).pow(2))
                    .sum::<i32>()
            })
            .unwrap()
            .0 as u8
    } else {
        let r = (rgb[0] as u16 * 7 + 127) / 255;
        let g = (rgb[1] as u16 * 7 + 127) / 255;
        let b = (rgb[2] as u16 * 3 + 127) / 255;
        ((r << 5) | (g << 2) | b) as u8
    }
}

fn palette_rgb(index: u8, four_bit: bool) -> [u8; 3] {
    if four_bit {
        PALETTE16[index as usize]
    } else {
        [
            (((index >> 5) as u16 * 255 + 3) / 7) as u8,
            ((((index >> 2) & 7) as u16 * 255 + 3) / 7) as u8,
            (((index & 3) as u16 * 255 + 1) / 3) as u8,
        ]
    }
}

fn quantize(image: &mut EditedImage) {
    use ColorDepth::*;
    if image.color_depth == Rgba8 {
        return;
    }
    for pixel in image.rgba.chunks_exact_mut(4) {
        let rgb = rgb_on_white(pixel);
        let converted = match image.color_depth {
            BlackWhite1 => [if luma(&rgb) >= 128 { 255 } else { 0 }; 3],
            Gray4 => [level(luma(&rgb), 15); 3],
            Gray8 | Gray16 => [luma(&rgb); 3],
            Indexed4 | Indexed8 => palette_rgb(
                palette_index(rgb, image.color_depth == Indexed4),
                image.color_depth == Indexed4,
            ),
            Rgb15 => rgb.map(|value| level(value, 31)),
            _ => rgb,
        };
        pixel[..3].copy_from_slice(&converted);
        pixel[3] = 255;
    }
}

pub fn apply(mut image: EditedImage, edit: &Edit) -> Result<EditedImage, String> {
    validate(&image)?;
    if crate::precision::is_native(edit) {
        let mut image = crate::precision::apply_native(image, edit)?;
        quantize(&mut image);
        return Ok(image);
    }
    match *edit {
        Edit::Exposure { ev } => {
            let gain = 2.0_f32.powf(ev);
            if !ev.is_finite() || !gain.is_finite() {
                return Err("曝光EV必须是有效有限数值".into());
            }
            let lookup: [u8; 256] = std::array::from_fn(|i| {
                let v = i as f32 / 255.0;
                let linear = if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                };
                let v = (linear * gain).clamp(0.0, 1.0);
                let srgb = if v <= 0.0031308 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (srgb * 255.0).round().clamp(0.0, 255.0) as u8
            });
            for pixel in image.rgba.chunks_exact_mut(4) {
                for channel in &mut pixel[..3] {
                    *channel = lookup[*channel as usize];
                }
            }
        }
        Edit::Grayscale => {
            for pixel in image.rgba.chunks_exact_mut(4) {
                let gray = luma(pixel);
                pixel[..3].fill(gray);
            }
        }
        Edit::ColorDepth { mode } => {
            bytes(
                image.width,
                image.height,
                if mode == ColorDepth::Rgb48 { 6 } else { 4 },
            )?;
            image.color_depth = mode;
        }
        _ => {
            let (width, height) = match *edit {
                Edit::Resize { width, height } => (width, height),
                Edit::Crop {
                    x,
                    y,
                    width,
                    height,
                } => {
                    if x.checked_add(width).is_none_or(|end| end > image.width)
                        || y.checked_add(height).is_none_or(|end| end > image.height)
                    {
                        return Err("裁剪区域超出图片范围".into());
                    }
                    (width, height)
                }
                Edit::Rotate90 { turns } if turns % 2 == 1 => (image.height, image.width),
                _ => (image.width, image.height),
            };
            bytes(
                width,
                height,
                if image.color_depth == ColorDepth::Rgb48 {
                    6
                } else {
                    4
                },
            )?;
            let mut source =
                RgbaImage::from_raw(image.width, image.height, image.rgba).ok_or("RGBA尺寸无效")?;
            let result = match *edit {
                // Integer box/interpolation resizing avoids a full RGBA32f intermediate.
                Edit::Resize { width, height } => {
                    image::imageops::thumbnail(&source, width, height)
                }
                Edit::Crop {
                    x,
                    y,
                    width,
                    height,
                } => image::imageops::crop_imm(&source, x, y, width, height).to_image(),
                Edit::Rotate90 { turns } => match turns % 4 {
                    1 => image::imageops::rotate90(&source),
                    2 => image::imageops::rotate180(&source),
                    3 => image::imageops::rotate270(&source),
                    _ => source,
                },
                Edit::FlipH => {
                    image::imageops::flip_horizontal_in_place(&mut source);
                    source
                }
                Edit::FlipV => {
                    image::imageops::flip_vertical_in_place(&mut source);
                    source
                }
                _ => unreachable!(),
            };
            image.width = width;
            image.height = height;
            image.rgba = result.into_raw();
        }
    }
    quantize(&mut image);
    Ok(image)
}

pub fn histogram(width: u32, height: u32, rgba: &[u8]) -> Result<[[u64; 256]; 3], String> {
    if bytes(width, height, 4)? != rgba.len() {
        return Err("直方图RGBA长度与尺寸不一致".into());
    }
    let mut bins = [[0u64; 256]; 3];
    for pixel in rgba.chunks_exact(4) {
        for channel in 0..3 {
            bins[channel][pixel[channel] as usize] += 1;
        }
    }
    Ok(bins)
}

fn samples(image: &EditedImage) -> Result<(Vec<u8>, ExtendedColorType), String> {
    use ColorDepth::*;
    let channels = match image.color_depth {
        Gray8 | Gray16 => 1,
        _ => 3,
    };
    let sixteen = matches!(image.color_depth, Gray16 | Rgb48);
    let mut output = Vec::with_capacity(bytes(
        image.width,
        image.height,
        channels * if sixteen { 2 } else { 1 },
    )?);
    for pixel in image.rgba.chunks_exact(4) {
        let rgb = rgb_on_white(pixel);
        if channels == 1 {
            let value = luma(&rgb);
            output.push(value);
            if sixteen {
                output.push(value);
            }
        } else {
            for value in rgb {
                output.push(value);
                if sixteen {
                    output.push(value);
                }
            }
        }
    }
    let color = match (channels, sixteen) {
        (1, false) => ExtendedColorType::L8,
        (1, true) => ExtendedColorType::L16,
        (_, true) => ExtendedColorType::Rgb16,
        _ => ExtendedColorType::Rgb8,
    };
    Ok((output, color))
}

fn write_png(image: &EditedImage, file: &mut fs::File) -> Result<(), String> {
    use ColorDepth::*;
    let mut encoder = png::Encoder::new(file, image.width, image.height);
    let mut packed = Vec::new();
    let data: &[u8];
    match image.color_depth {
        BlackWhite1 | Gray4 | Indexed4 | Indexed8 => {
            let bits = match image.color_depth {
                BlackWhite1 => 1,
                Gray4 | Indexed4 => 4,
                _ => 8,
            };
            let indexed = matches!(image.color_depth, Indexed4 | Indexed8);
            encoder.set_color(if indexed {
                png::ColorType::Indexed
            } else {
                png::ColorType::Grayscale
            });
            encoder.set_depth(match bits {
                1 => png::BitDepth::One,
                4 => png::BitDepth::Four,
                _ => png::BitDepth::Eight,
            });
            if indexed {
                let colors = if bits == 4 { 16 } else { 256 };
                let palette: Vec<u8> = (0..colors)
                    .flat_map(|i| palette_rgb(i as u8, bits == 4))
                    .collect();
                encoder.set_palette(palette);
            }
            let row_bytes = (image.width as usize * bits).div_ceil(8);
            packed.resize(row_bytes * image.height as usize, 0);
            for y in 0..image.height as usize {
                for x in 0..image.width as usize {
                    let rgb = rgb_on_white(&image.rgba[(y * image.width as usize + x) * 4..][..4]);
                    let index = if indexed {
                        palette_index(rgb, bits == 4)
                    } else {
                        ((luma(&rgb) as usize * ((1 << bits) - 1) + 127) / 255) as u8
                    };
                    packed[y * row_bytes + x * bits / 8] |= index << (8 - bits - x * bits % 8);
                }
            }
            data = &packed;
        }
        Rgba8 => {
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            data = &image.rgba;
        }
        _ => {
            let (buffer, color) = samples(image)?;
            packed = buffer;
            encoder.set_color(
                if matches!(color, ExtendedColorType::L8 | ExtendedColorType::L16) {
                    png::ColorType::Grayscale
                } else {
                    png::ColorType::Rgb
                },
            );
            encoder.set_depth(
                if matches!(color, ExtendedColorType::L16 | ExtendedColorType::Rgb16) {
                    png::BitDepth::Sixteen
                } else {
                    png::BitDepth::Eight
                },
            );
            data = &packed;
        }
    }
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(data).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

fn write_rgb555(image: &EditedImage, file: &mut fs::File) -> Result<(), String> {
    bytes(image.width, image.height, 2)?;
    let row_bytes = (image.width as usize * 2).div_ceil(4) * 4;
    let size = 54usize
        .checked_add(row_bytes * image.height as usize)
        .ok_or("BMP尺寸溢出")?;
    let mut header = vec![0u8; 54];
    header[..2].copy_from_slice(b"BM");
    header[2..6].copy_from_slice(&(size as u32).to_le_bytes());
    header[10..14].copy_from_slice(&54u32.to_le_bytes());
    header[14..18].copy_from_slice(&40u32.to_le_bytes());
    header[18..22].copy_from_slice(&image.width.to_le_bytes());
    header[22..26].copy_from_slice(&image.height.to_le_bytes());
    header[26..28].copy_from_slice(&1u16.to_le_bytes());
    header[28..30].copy_from_slice(&16u16.to_le_bytes());
    file.write_all(&header).map_err(|e| e.to_string())?;
    let mut row = vec![0u8; row_bytes];
    for y in (0..image.height as usize).rev() {
        for x in 0..image.width as usize {
            let rgb = rgb_on_white(&image.rgba[(y * image.width as usize + x) * 4..][..4]);
            let q = rgb.map(|v| (v as u16 * 31 + 127) / 255);
            let value = (q[0] << 10) | (q[1] << 5) | q[2];
            row[x * 2..x * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        file.write_all(&row).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Encode first, preserve the first original, then atomically replace the target.
/// RAW is never written. Indexed/low-bit modes require PNG; RGB555 requires BMP.
pub fn save_to(image: &EditedImage, path: &Path) -> Result<(), String> {
    validate(image)?;
    let format = ImageFormat::from_path(path)
        .map_err(|_| "请选择可编码的PNG/JPEG/TIFF/BMP/WebP等格式；RAW不回写".to_owned())?;
    use ColorDepth::*;
    if matches!(image.color_depth, BlackWhite1 | Gray4 | Indexed4 | Indexed8)
        && format != ImageFormat::Png
    {
        return Err("精确低位/索引色保存请选择PNG".into());
    }
    if image.color_depth == Rgb15 && format != ImageFormat::Bmp {
        return Err("精确15位RGB555保存请选择BMP".into());
    }
    if matches!(image.color_depth, Gray16 | Rgb48)
        && !matches!(format, ImageFormat::Png | ImageFormat::Tiff)
    {
        return Err("16/48位样本保存请选择PNG或TIFF；RGBA8输入不恢复原始高位精度".into());
    }
    if format == ImageFormat::Gif {
        return Err("静态编辑不回写GIF动画，请另存为PNG".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".rustview-save-")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    if format == ImageFormat::Png {
        write_png(image, temporary.as_file_mut())?;
    } else if image.color_depth == Rgb15 {
        write_rgb555(image, temporary.as_file_mut())?;
    } else if image.color_depth == Rgba8
        && format != ImageFormat::Jpeg
        && format != ImageFormat::Pnm
    {
        image::write_buffer_with_format(
            temporary.as_file_mut(),
            &image.rgba,
            image.width,
            image.height,
            ExtendedColorType::Rgba8,
            format,
        )
        .map_err(|e| e.to_string())?;
    } else {
        let (data, color) = samples(image)?;
        image::write_buffer_with_format(
            temporary.as_file_mut(),
            &data,
            image.width,
            image.height,
            color,
            format,
        )
        .map_err(|e| e.to_string())?;
    }
    publish(temporary, path)
}

/// Publish only a complete encoded file; a failed render/backup never truncates its source.
pub(crate) fn publish(temporary: tempfile::NamedTempFile, path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    if path.exists() {
        if !path.is_file() {
            return Err("保存目标不是普通文件".into());
        }
        let originals = parent.join(".rustview-originals");
        fs::create_dir_all(&originals).map_err(|e| e.to_string())?;
        let backup = originals.join(path.file_name().ok_or("保存路径没有文件名")?);
        if !backup.is_file() {
            let mut saved = tempfile::Builder::new()
                .prefix(".rustview-backup-")
                .tempfile_in(&originals)
                .map_err(|e| format!("无法保留原图：{e}"))?;
            fs::File::open(path)
                .and_then(|mut source| io::copy(&mut source, saved.as_file_mut()))
                .and_then(|_| saved.as_file().sync_all())
                .map_err(|e| format!("无法保留原图：{e}"))?;
            match saved.persist_noclobber(&backup) {
                Ok(_) => {}
                Err(error)
                    if error.error.kind() == io::ErrorKind::AlreadyExists && backup.is_file() => {}
                Err(error) => return Err(format!("无法保留原图：{error}")),
            }
        }
    }
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pixels(width: u32, height: u32, rgba: Vec<u8>) -> EditedImage {
        EditedImage {
            width,
            height,
            rgba,
            color_depth: ColorDepth::Rgba8,
        }
    }

    #[test]
    fn exposure_is_linear_light_and_preserves_alpha() {
        let result = apply(
            pixels(1, 1, vec![128, 128, 128, 17]),
            &Edit::Exposure { ev: 1.0 },
        )
        .unwrap();
        assert!((result.rgba[0] as i16 - 176).abs() <= 1);
        assert_eq!(result.rgba[3], 17);
        assert!(
            apply(
                pixels(1, 1, vec![0, 0, 0, 255]),
                &Edit::Exposure { ev: f32::NAN }
            )
            .is_err()
        );
    }

    #[test]
    fn rotation_and_crop_use_actual_pixels() {
        let result = apply(
            pixels(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]),
            &Edit::Rotate90 { turns: 1 },
        )
        .unwrap();
        assert_eq!((result.width, result.height), (1, 2));
        let result = apply(
            result,
            &Edit::Crop {
                x: 0,
                y: 1,
                width: 1,
                height: 1,
            },
        )
        .unwrap();
        assert_eq!(result.rgba, vec![0, 255, 0, 255]);
        assert!(
            apply(
                result,
                &Edit::Resize {
                    width: u32::MAX,
                    height: u32::MAX
                }
            )
            .is_err()
        );
        assert!(
            apply(
                pixels(1, 1, vec![0, 0, 0, 255]),
                &Edit::Crop {
                    x: 1,
                    y: 0,
                    width: 1,
                    height: 1
                }
            )
            .is_err()
        );
    }

    #[test]
    fn png_depths_are_real_and_survive_later_edits() {
        let dir = tempfile::tempdir().unwrap();
        for (mode, bits, color) in [
            (
                ColorDepth::BlackWhite1,
                png::BitDepth::One,
                png::ColorType::Grayscale,
            ),
            (
                ColorDepth::Gray4,
                png::BitDepth::Four,
                png::ColorType::Grayscale,
            ),
            (
                ColorDepth::Indexed4,
                png::BitDepth::Four,
                png::ColorType::Indexed,
            ),
            (
                ColorDepth::Indexed8,
                png::BitDepth::Eight,
                png::ColorType::Indexed,
            ),
            (
                ColorDepth::Rgb48,
                png::BitDepth::Sixteen,
                png::ColorType::Rgb,
            ),
        ] {
            let result = apply(
                pixels(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]),
                &Edit::ColorDepth { mode },
            )
            .unwrap();
            let result = apply(result, &Edit::FlipH).unwrap();
            assert_eq!(result.color_depth, mode);
            let path = dir.path().join(format!("{mode:?}.png"));
            save_to(&result, &path).unwrap();
            let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
            let reader = decoder.read_info().unwrap();
            assert_eq!(reader.info().bit_depth, bits);
            assert_eq!(reader.info().color_type, color);
        }
    }

    #[test]
    fn backup_retains_first_original_and_failure_keeps_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        fs::write(&path, b"original bytes").unwrap();
        let image = pixels(1, 1, vec![1, 2, 3, 255]);
        save_to(&image, &path).unwrap();
        save_to(&image, &path).unwrap();
        assert_eq!(
            fs::read(dir.path().join(".rustview-originals/photo.png")).unwrap(),
            b"original bytes"
        );
        let before = fs::read(&path).unwrap();
        let image = apply(
            image,
            &Edit::ColorDepth {
                mode: ColorDepth::Rgb15,
            },
        )
        .unwrap();
        assert!(save_to(&image, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let bmp = dir.path().join("rgb555.bmp");
        save_to(&image, &bmp).unwrap();
        assert_eq!(&fs::read(bmp).unwrap()[28..30], &16u16.to_le_bytes());
    }

    #[test]
    fn interrupted_backup_staging_is_never_trusted_and_backup_failure_keeps_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let originals = dir.path().join(".rustview-originals");
        fs::create_dir(&originals).unwrap();
        let orphan = originals.join(".rustview-backup-interrupted");
        fs::write(&orphan, b"partial").unwrap();
        fs::write(&path, b"complete original").unwrap();
        let image = pixels(1, 1, vec![1, 2, 3, 255]);
        save_to(&image, &path).unwrap();
        assert_eq!(
            fs::read(originals.join("photo.png")).unwrap(),
            b"complete original"
        );
        assert_eq!(fs::read(&orphan).unwrap(), b"partial");
        let blocked = dir.path().join("blocked.png");
        fs::write(&blocked, b"do not overwrite").unwrap();
        fs::create_dir(originals.join("blocked.png")).unwrap();
        assert!(save_to(&image, &blocked).is_err());
        assert_eq!(fs::read(&blocked).unwrap(), b"do not overwrite");
        assert_eq!(fs::read_dir(&originals).unwrap().count(), 3);
    }

    #[test]
    fn histogram_counts_each_channel_without_storing_images() {
        let bins = histogram(2, 1, &[1, 2, 3, 255, 1, 9, 3, 255]).unwrap();
        assert_eq!(bins[0][1], 2);
        assert_eq!(bins[1][2], 1);
        assert_eq!(bins[2][3], 2);
    }
}
