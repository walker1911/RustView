use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use image::{AnimationDecoder, ImageDecoder, Limits};

const MAX_FRAME_BYTES: usize = 128 * 1024 * 1024;
const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;

/// Finite counts are total plays, including the initial play, for both formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RepeatCount {
    Infinite,
    Finite(u32),
}

pub struct AnimationFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub duration_ms: u32,
}

pub struct AnimationStream {
    path: PathBuf,
    frames: image::Frames<'static>,
    max_frame_bytes: usize,
    repeat: RepeatCount,
    orientation: image::metadata::Orientation,
    icc_profile: Option<Vec<u8>>,
    color_managed: bool,
    ended: bool,
}

fn check_size(width: u32, height: u32, limit: usize) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| width > 0 && height > 0 && *bytes <= limit)
        .ok_or_else(|| format!("动画画布 {width} × {height} 超过单帧内存预算或尺寸无效"))
}

fn decoder_limits(max_frame_bytes: usize) -> Limits {
    let mut limits = Limits::default();
    // Disposal/compositing needs a persistent canvas, a subframe, and an output canvas.
    limits.max_alloc = Some((max_frame_bytes as u64) * 3);
    limits.max_image_width = Some((max_frame_bytes / 4) as u32);
    limits.max_image_height = limits.max_image_width;
    limits
}

fn gif_metadata(path: &Path, max_frame_bytes: usize) -> Result<(u32, RepeatCount), String> {
    let mut options = gif::DecodeOptions::new();
    options.skip_frame_decoding(true);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        NonZeroU64::new(4 * 1024 * 1024).unwrap(),
    ));
    let mut decoder = options
        .read_info(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
        .map_err(|e| format!("GIF 元数据读取失败：{e}"))?;
    let dimensions = (u32::from(decoder.width()), u32::from(decoder.height()));
    let repeat = match decoder.repeat() {
        gif::Repeat::Infinite => RepeatCount::Infinite,
        gif::Repeat::Finite(repeats) => RepeatCount::Finite(u32::from(repeats) + 1),
    };
    let mut count = 0_u32;
    let mut largest_subframe = 0_u64;
    // This pass skips compressed image data; it does not allocate decoded frames.
    while let Some(frame) = decoder.next_frame_info().map_err(|e| e.to_string())? {
        largest_subframe =
            largest_subframe.max(u64::from(frame.width) * u64::from(frame.height) * 4);
        count = count.checked_add(1).ok_or("GIF 帧数溢出")?;
    }
    if count >= 2 {
        check_size(dimensions.0, dimensions.1, max_frame_bytes)?;
        if largest_subframe > max_frame_bytes as u64 {
            return Err("GIF 子帧超过动画单帧内存预算".into());
        }
    }
    Ok((count, repeat))
}

pub fn open(path: &Path, max_frame_bytes: usize) -> Result<Option<AnimationStream>, String> {
    let mut file = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    if file.get_ref().metadata().map_err(|e| e.to_string())?.len() > MAX_INPUT_BYTES {
        return Err("动画输入超过 512 MiB".into());
    }
    let mut signature = [0_u8; 12];
    let count = file.read(&mut signature).map_err(|e| e.to_string())?;
    let gif = count >= 6 && matches!(&signature[..6], b"GIF87a" | b"GIF89a");
    let webp = count == 12 && &signature[..4] == b"RIFF" && &signature[8..12] == b"WEBP";
    if !gif && !webp {
        return Ok(None);
    }
    let max_frame_bytes = max_frame_bytes.min(MAX_FRAME_BYTES);
    if max_frame_bytes < 4 {
        return Err("动画单帧预算至少需要 4 字节".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let (frames, repeat, orientation, icc_profile) = if gif {
        let (frame_count, repeat) = gif_metadata(path, max_frame_bytes)?;
        if frame_count < 2 {
            return Ok(None);
        }
        let mut decoder =
            image::codecs::gif::GifDecoder::new(file).map_err(|e| format!("GIF 打开失败：{e}"))?;
        let (width, height) = decoder.dimensions();
        check_size(width, height, max_frame_bytes)?;
        decoder
            .set_limits(decoder_limits(max_frame_bytes))
            .map_err(|e| e.to_string())?;
        let profile = decoder
            .icc_profile()
            .ok()
            .flatten()
            .filter(|bytes| bytes.len() <= 4 * 1024 * 1024);
        (
            decoder.into_frames(),
            repeat,
            image::metadata::Orientation::NoTransforms,
            profile,
        )
    } else {
        let metadata = image_webp::WebPDecoder::new(BufReader::new(
            File::open(path).map_err(|e| e.to_string())?,
        ))
        .map_err(|e| format!("WebP 元数据读取失败：{e}"))?;
        let (width, height) = metadata.dimensions();
        if !metadata.is_animated() || metadata.num_frames() < 2 {
            return Ok(None);
        }
        check_size(width, height, max_frame_bytes)?;
        let repeat = match metadata.loop_count() {
            image_webp::LoopCount::Forever => RepeatCount::Infinite,
            image_webp::LoopCount::Times(count) => RepeatCount::Finite(u32::from(count.get())),
        };
        // ANIM stores BGRA bytes; the image decoder's canvas setter takes RGBA.
        let background = metadata
            .background_color_hint()
            .map(|bgra| [bgra[2], bgra[1], bgra[0], bgra[3]]);
        drop(metadata);
        let mut decoder = image::codecs::webp::WebPDecoder::new(file)
            .map_err(|e| format!("WebP 打开失败：{e}"))?;
        let (width, height) = decoder.dimensions();
        check_size(width, height, max_frame_bytes)?;
        decoder
            .set_limits(decoder_limits(max_frame_bytes))
            .map_err(|e| e.to_string())?;
        if let Some(background) = background {
            decoder
                .set_background_color(image::Rgba(background))
                .map_err(|e| e.to_string())?;
        }
        let orientation = decoder
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let profile = decoder
            .icc_profile()
            .ok()
            .flatten()
            .filter(|bytes| bytes.len() <= 4 * 1024 * 1024);
        (decoder.into_frames(), repeat, orientation, profile)
    };
    Ok(Some(AnimationStream {
        path: path.to_owned(),
        frames,
        max_frame_bytes,
        repeat,
        orientation,
        icc_profile,
        color_managed: true,
        ended: false,
    }))
}

impl AnimationStream {
    /// Configure before reading frames; callers reopen the stream when changing this setting.
    pub fn set_color_management(&mut self, enabled: bool) {
        self.color_managed = enabled;
        if !enabled {
            self.icc_profile = None;
        }
    }

    pub fn repeat(&self) -> RepeatCount {
        self.repeat
    }

    pub fn next_frame(&mut self) -> Result<Option<AnimationFrame>, String> {
        if self.ended {
            return Ok(None);
        }
        let frame = match self.frames.next() {
            Some(Ok(frame)) => frame,
            Some(Err(error)) => {
                self.ended = true;
                return Err(format!("动画帧解码失败：{error}"));
            }
            None => {
                self.ended = true;
                return Ok(None);
            }
        };
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let duration_ms = u64::from(numerator)
            .div_ceil(u64::from(denominator.max(1)))
            .clamp(10, u64::from(u32::MAX)) as u32;
        let buffer = frame.into_buffer();
        check_size(buffer.width(), buffer.height(), self.max_frame_bytes)?;
        let mut image = image::DynamicImage::ImageRgba8(buffer);
        image.apply_orientation(self.orientation);
        let mut image = image.into_rgba8();
        let expected = check_size(image.width(), image.height(), self.max_frame_bytes)?;
        if image.len() != expected {
            self.ended = true;
            return Err("动画帧尺寸和像素长度不一致".into());
        }
        if let Some(profile) = self.icc_profile.as_ref() {
            if crate::imaging::apply_icc(profile, image.as_mut()).is_err() {
                self.icc_profile = None;
            }
        }
        Ok(Some(AnimationFrame {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
            duration_ms,
        }))
    }

    pub fn reset(&mut self) -> Result<(), String> {
        let mut replacement =
            open(&self.path, self.max_frame_bytes)?.ok_or("文件已不再包含多帧动画")?;
        replacement.set_color_management(self.color_managed);
        *self = replacement;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn pixel(frame: &AnimationFrame, x: usize, y: usize) -> [u8; 4] {
        let position = (y * frame.width as usize + x) * 4;
        frame.rgba[position..position + 4].try_into().unwrap()
    }

    #[test]
    fn gif_disposal_previous_transparency_delays_and_reset() {
        let mut stream = open(&fixture("animation-previous.gif"), 32)
            .unwrap()
            .unwrap();
        assert_eq!(stream.repeat(), RepeatCount::Finite(2));
        let first = stream.next_frame().unwrap().unwrap();
        assert_eq!(first.duration_ms, 30);
        assert_eq!(pixel(&first, 0, 0), [255, 0, 0, 255]);
        let second = stream.next_frame().unwrap().unwrap();
        assert_eq!(second.duration_ms, 50);
        assert_eq!(pixel(&second, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&second, 1, 0), [255, 0, 0, 255]);
        let third = stream.next_frame().unwrap().unwrap();
        assert_eq!(third.duration_ms, 10);
        assert_eq!(pixel(&third, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&third, 2, 0), [0, 255, 0, 255]);
        assert!(stream.next_frame().unwrap().is_none());
        stream.reset().unwrap();
        assert_eq!(stream.next_frame().unwrap().unwrap().rgba, first.rgba);
    }

    #[test]
    fn gif_disposal_background_clears_previous_rectangle() {
        let mut stream = open(&fixture("animation-background.gif"), 32)
            .unwrap()
            .unwrap();
        assert_eq!(stream.repeat(), RepeatCount::Infinite);
        stream.next_frame().unwrap();
        stream.next_frame().unwrap();
        let third = stream.next_frame().unwrap().unwrap();
        assert_eq!(pixel(&third, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&third, 2, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn webp_blending_disposal_delays_and_total_plays() {
        let mut stream = open(&fixture("animation-blend.webp"), 32).unwrap().unwrap();
        assert_eq!(stream.repeat(), RepeatCount::Finite(2));
        let first = stream.next_frame().unwrap().unwrap();
        assert_eq!(first.duration_ms, 30);
        assert_eq!(pixel(&first, 0, 0), [255, 0, 0, 255]);
        let second = stream.next_frame().unwrap().unwrap();
        assert_eq!(second.duration_ms, 50);
        let blended = pixel(&second, 0, 0);
        assert!((126..=128).contains(&blended[0]));
        assert!((127..=129).contains(&blended[2]));
        assert_eq!(blended[3], 255);
        assert_eq!(pixel(&second, 2, 0), [255, 0, 0, 255]);
        let third = stream.next_frame().unwrap().unwrap();
        assert_eq!(third.duration_ms, 10);
        assert_eq!(pixel(&third, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&third, 2, 0), [0, 255, 0, 255]);
        assert!(stream.next_frame().unwrap().is_none());
        stream.reset().unwrap();
        assert_eq!(stream.next_frame().unwrap().unwrap().rgba, first.rgba);
    }

    #[test]
    fn canvas_budget_rejected_before_frame_allocation() {
        assert!(open(&fixture("animation-previous.gif"), 31).is_err());
        assert!(open(&fixture("animation-blend.webp"), 31).is_err());
        assert!(check_size(u32::MAX, u32::MAX, MAX_FRAME_BYTES).is_err());
    }

    #[test]
    fn static_webp_does_not_start_an_animation() {
        assert!(open(&fixture("test.webp"), 128).unwrap().is_none());
    }

    fn webp_chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut chunk = id.to_vec();
        chunk.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        chunk.extend_from_slice(payload);
        if payload.len() % 2 != 0 {
            chunk.push(0);
        }
        chunk
    }

    #[test]
    fn webp_disposal_uses_anim_background_color() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("background.webp");
        let mut bytes = std::fs::read(fixture("animation-blend.webp")).unwrap();
        let anim = bytes.windows(4).position(|part| part == b"ANIM").unwrap();
        bytes[anim + 8..anim + 12].copy_from_slice(&[30, 20, 10, 255]);
        bytes[anim + 12..anim + 14].fill(0);
        std::fs::write(&path, bytes).unwrap();
        let mut stream = open(&path, 32).unwrap().unwrap();
        assert_eq!(stream.repeat(), RepeatCount::Infinite);
        stream.next_frame().unwrap();
        stream.next_frame().unwrap();
        let third = stream.next_frame().unwrap().unwrap();
        assert_eq!(pixel(&third, 0, 0), [10, 20, 30, 255]);
    }

    #[test]
    fn webp_applies_exif_and_icc_to_every_frame() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile-rotated.webp");
        let profile = moxcms::ColorProfile::new_adobe_rgb().encode().unwrap();
        let mut bytes = std::fs::read(fixture("animation-blend.webp")).unwrap();
        bytes.splice(30..30, webp_chunk(b"ICCP", &profile));
        let exif = [
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ];
        bytes.extend(webp_chunk(b"EXIF", &exif));
        bytes[20] |= 0x28;
        let length = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        let mut actual = open(&path, 32).unwrap().unwrap();
        let mut reference = open(&fixture("animation-blend.webp"), 32).unwrap().unwrap();
        for _ in 0..3 {
            let before = reference.next_frame().unwrap().unwrap();
            let mut expected = image::DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(before.width, before.height, before.rgba).unwrap(),
            );
            expected.apply_orientation(image::metadata::Orientation::Rotate90);
            let mut expected = expected.into_rgba8();
            crate::imaging::apply_icc(&profile, expected.as_mut()).unwrap();
            let decoded = actual.next_frame().unwrap().unwrap();
            assert_eq!((decoded.width, decoded.height), (2, 4));
            assert_eq!(decoded.rgba, expected.into_raw());
        }
        actual.set_color_management(false);
        for _ in 0..2 {
            actual.reset().unwrap();
            reference.reset().unwrap();
            assert!(!actual.color_managed);
            assert!(actual.icc_profile.is_none());
            for _ in 0..3 {
                let before = reference.next_frame().unwrap().unwrap();
                let mut expected = image::DynamicImage::ImageRgba8(
                    image::RgbaImage::from_raw(before.width, before.height, before.rgba).unwrap(),
                );
                expected.apply_orientation(image::metadata::Orientation::Rotate90);
                assert_eq!(
                    actual.next_frame().unwrap().unwrap().rgba,
                    expected.into_rgba8().into_raw()
                );
            }
        }
    }

    #[test]
    fn gif_without_loop_extension_plays_once_and_static_is_not_animated() {
        let directory = tempfile::tempdir().unwrap();
        for count in [1, 2] {
            let path = directory.path().join(format!("{count}.gif"));
            let file = File::create(&path).unwrap();
            {
                let mut encoder = gif::Encoder::new(file, 1, 1, &[255, 0, 0]).unwrap();
                for _ in 0..count {
                    encoder
                        .write_frame(&gif::Frame {
                            width: 1,
                            height: 1,
                            buffer: std::borrow::Cow::Borrowed(&[0]),
                            ..Default::default()
                        })
                        .unwrap();
                }
            }
            let stream = open(&path, 4).unwrap();
            if count == 1 {
                assert!(stream.is_none());
            } else {
                assert_eq!(stream.unwrap().repeat(), RepeatCount::Finite(1));
            }
        }
    }

    #[test]
    fn hard_frame_cap_applies_even_with_unlimited_caller_budget() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.gif");
        let mut bytes = std::fs::read(fixture("animation-previous.gif")).unwrap();
        bytes[6..8].copy_from_slice(&20_000_u16.to_le_bytes());
        bytes[8..10].copy_from_slice(&20_000_u16.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(open(&path, usize::MAX).is_err());
    }
}
