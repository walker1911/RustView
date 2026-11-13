use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read, Seek};
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub note: String,
}

pub(crate) struct JpegPreview {
    pub image: DecodedImage,
    pub source_width: u32,
    pub source_height: u32,
}

/// Try a native JPEG preview. The normal decoder remains authoritative for
/// unsupported JPEGs and for the full image.
pub(crate) fn decode_jpeg_preview(
    input: &Path,
    max_side: u32,
    min_pixels: u64,
    max_pixels: u64,
    max_output_bytes: usize,
    managed: bool,
) -> Option<JpegPreview> {
    #[cfg(windows)]
    {
        if max_side == 0
            || !matches!(
                extension(&input.to_string_lossy()).as_str(),
                "jpg" | "jpeg" | "jpe" | "jfif"
            )
        {
            return None;
        }
        jpeg_preview::decode(input, max_side, min_pixels, max_pixels, max_output_bytes, managed)
    }
    #[cfg(not(windows))]
    {
        let _ = (input, max_side, min_pixels, max_pixels, max_output_bytes, managed);
        None
    }
}

/// Decode only the JPEG embedded in a RAW file. Run inside a supervised worker:
/// LibRaw may allocate its own thumbnail buffers before returning their size.
pub(crate) fn decode_raw_preview(
    input: &Path,
    max_side: u32,
    max_pixels: u64,
    max_output_bytes: usize,
) -> Option<JpegPreview> {
    #[cfg(windows)]
    {
        if max_side == 0 || !is_raw(&input.to_string_lossy()) {
            return None;
        }
        checked_input_size(input).ok()?;
        raw::decode_preview(input, max_side, max_pixels, max_output_bytes)
    }
    #[cfg(not(windows))]
    {
        let _ = (input, max_side, max_pixels, max_output_bytes);
        None
    }
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn is_raw(name: &str) -> bool {
    matches!(
        extension(name).as_str(),
        "3fr"
            | "arw"
            | "cr2"
            | "cr3"
            | "crw"
            | "dcr"
            | "dcs"
            | "dng"
            | "erf"
            | "fff"
            | "gpr"
            | "iiq"
            | "kdc"
            | "mef"
            | "mos"
            | "mrw"
            | "nef"
            | "nrw"
            | "orf"
            | "pef"
            | "raf"
            | "raw"
            | "rw2"
            | "rwl"
            | "sr2"
            | "srf"
            | "srw"
            | "x3f"
    )
}

pub fn is_photoshop(name: &str) -> bool {
    matches!(extension(name).as_str(), "psd" | "psb")
}

const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PHOTOSHOP_INPUT_BYTES: u64 = 64 * 1024 * 1024 * 1024;

pub fn input_byte_limit(name: &str) -> u64 {
    if is_photoshop(name) {
        MAX_PHOTOSHOP_INPUT_BYTES
    } else {
        MAX_INPUT_BYTES
    }
}

pub(crate) fn checked_input_size(input: &Path) -> Result<u64, String> {
    let metadata = input
        .metadata()
        .map_err(|error| format!("无法打开图像：{error}"))?;
    if !metadata.is_file() {
        return Err("图像来源不是普通文件".into());
    }
    let size = metadata.len();
    let limit = input_byte_limit(&input.to_string_lossy());
    if size > limit {
        return Err(if limit == MAX_PHOTOSHOP_INPUT_BYTES {
            "PSD/PSB输入超过64 GiB，已停止读取".into()
        } else {
            "输入文件超过512 MiB，已停止读取".into()
        });
    }
    if size > MAX_INPUT_BYTES {
        let mut header = [0; 6];
        File::open(input)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|error| format!("无法读取PSD/PSB文件头：{error}"))?;
        if &header[..4] != b"8BPS" || !matches!(u16::from_be_bytes([header[4], header[5]]), 1 | 2) {
            return Err("超过512 MiB的输入仅接受有效PSD/PSB文件头，不能仅改文件后缀".into());
        }
    }
    Ok(size)
}

pub fn is_supported_image(name: &str) -> bool {
    is_raw(name)
        || is_photoshop(name)
        || matches!(
            extension(name).as_str(),
            "jpg"
                | "jpeg"
                | "jpe"
                | "jfif"
                | "png"
                | "webp"
                | "gif"
                | "bmp"
                | "dib"
                | "tif"
                | "tiff"
                | "ico"
                | "pnm"
                | "pbm"
                | "pgm"
                | "ppm"
                | "pam"
                | "tga"
                | "qoi"
                | "heic"
                | "heif"
                | "hif"
                | "avif"
                | "jxl"
        )
}

fn output_size(
    width: u32,
    height: u32,
    max_pixels: u64,
    max_output_bytes: usize,
) -> Result<usize, String> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .filter(|pixels| *pixels > 0 && *pixels <= max_pixels)
        .ok_or_else(|| format!("图像尺寸 {width} × {height} 超过像素限制，或尺寸无效"))?;
    let bytes = pixels
        .checked_mul(4)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .filter(|bytes| *bytes <= max_output_bytes)
        .ok_or_else(|| format!("图像 {width} × {height} 超过单张 RGBA 内存预算"))?;
    Ok(bytes)
}

pub fn decode(
    input: &Path,
    max_pixels: u64,
    max_output_bytes: usize,
    raw_full: bool,
) -> Result<DecodedImage, String> {
    decode_options(input, max_pixels, max_output_bytes, raw_full, true)
}

pub fn decode_unmanaged(
    input: &Path,
    max_pixels: u64,
    max_output_bytes: usize,
    raw_full: bool,
) -> Result<DecodedImage, String> {
    decode_options(input, max_pixels, max_output_bytes, raw_full, false)
}

fn decode_options(
    input: &Path,
    max_pixels: u64,
    max_output_bytes: usize,
    raw_full: bool,
    managed: bool,
) -> Result<DecodedImage, String> {
    checked_input_size(input)?;
    if is_raw(&input.to_string_lossy()) {
        return decode_raw(input, max_pixels, max_output_bytes, raw_full);
    }
    if is_photoshop(&input.to_string_lossy()) {
        return match crate::psd_composite::inspect(input) {
            Ok(info) => crate::precision::decode_photoshop(input, &info, max_pixels, max_output_bytes, managed),
            Err(_) => crate::precision::decode_fallback(input, max_pixels, max_output_bytes, managed),
        };
    }
    let file = File::open(input).map_err(|error| format!("无法打开图像：{error}"))?;
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    // TGA has no reliable magic signature. Prefer detected content, then the extension.
    if reader.format().is_none() {
        if let Ok(format) = ImageFormat::from_path(input) {
            reader.set_format(format);
        }
    }
    decode_reader_options(reader, max_pixels, max_output_bytes, managed).or_else(|error| {
        if is_supported_image(&input.to_string_lossy()) {
            crate::precision::decode_fallback(input, max_pixels, max_output_bytes, managed)
                .map_err(|fallback| format!("{error}；备用解码：{fallback}"))
        } else {
            Err(error)
        }
    })
}

fn decode_reader<R: BufRead + Seek>(
    reader: ImageReader<R>,
    max_pixels: u64,
    max_output_bytes: usize,
) -> Result<DecodedImage, String> {
    decode_reader_options(reader, max_pixels, max_output_bytes, true)
}

fn decode_reader_options<R: BufRead + Seek>(
    mut reader: ImageReader<R>,
    max_pixels: u64,
    max_output_bytes: usize,
    managed: bool,
) -> Result<DecodedImage, String> {
    let format = reader.format();
    let mut limits = Limits::default();
    limits.max_alloc = Some(max_output_bytes as u64);
    limits.max_image_width = Some(max_pixels.min(u64::from(u32::MAX)) as u32);
    limits.max_image_height = limits.max_image_width;
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| format!("无法识别或打开图像：{error}"))?;
    let (width, height) = decoder.dimensions();
    output_size(width, height, max_pixels, max_output_bytes)?;

    let mut notes = Vec::new();
    let orientation = match decoder.orientation() {
        Ok(value) => value,
        Err(error) => {
            notes.push(format!("方向元数据无效：{error}"));
            image::metadata::Orientation::NoTransforms
        }
    };
    let profile = match decoder.icc_profile() {
        Ok(value) => value,
        Err(error) => {
            notes.push(format!("ICC 读取失败：{error}"));
            None
        }
    };
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|error| format!("图像解码失败：{error}"))?;
    image.apply_orientation(orientation);
    let mut image = image.into_rgba8();
    output_size(image.width(), image.height(), max_pixels, max_output_bytes)?;
    if let Some(profile) = profile.filter(|_| managed) {
        match apply_icc(&profile, image.as_mut()) {
            Ok(()) => notes.push("ICC → sRGB".into()),
            Err(error) => notes.push(format!("ICC 未应用：{error}")),
        }
    }
    match format {
        Some(ImageFormat::Gif) => notes.push("GIF：当前显示首帧".into()),
        Some(ImageFormat::WebP) => notes.push("WebP：当前显示首帧".into()),
        Some(ImageFormat::Tiff) => notes.push("TIFF：当前显示首图".into()),
        _ => {}
    }
    Ok(DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        note: notes.join(" · "),
    })
}

pub(crate) fn apply_icc(bytes: &[u8], rgba: &mut [u8]) -> Result<(), String> {
    apply_icc_to(bytes, rgba, &ColorProfile::new_srgb(), TransformOptions::default())
}

pub(crate) fn apply_icc_to(
    bytes: &[u8],
    rgba: &mut [u8],
    target: &ColorProfile,
    options: TransformOptions,
) -> Result<(), String> {
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("色彩配置超过 4 MiB".into());
    }
    let source = ColorProfile::new_from_slice(bytes).map_err(|error| error.to_string())?;
    let source_layout = match source.color_space {
        DataColorSpace::Rgb => Layout::Rgba,
        DataColorSpace::Gray => Layout::GrayAlpha,
        _ => return Err("当前只转换 RGB / 灰度 ICC，使用解码器提供的 RGB".into()),
    };
    let transform = source
        .create_transform_8bit(
            source_layout,
            target,
            Layout::Rgba,
            options,
        )
        .map_err(|error| error.to_string())?;
    // A fixed scratch buffer avoids allocating another full-resolution bitmap.
    let mut input = vec![0_u8; 4096 * 4];
    let mut output = vec![0_u8; 4096 * 4];
    for chunk in rgba.chunks_mut(4096 * 4) {
        let input_len = if source_layout == Layout::GrayAlpha {
            for (pixel, gray) in chunk.chunks_exact(4).zip(input.chunks_exact_mut(2)) {
                gray.copy_from_slice(&[pixel[0], pixel[3]]);
            }
            chunk.len() / 2
        } else {
            input[..chunk.len()].copy_from_slice(chunk);
            chunk.len()
        };
        transform
            .transform(&input[..input_len], &mut output[..chunk.len()])
            .map_err(|error| error.to_string())?;
        // Alpha is coverage, not a color channel; preserve it explicitly.
        for (pixel, original) in output[..chunk.len()]
            .chunks_exact_mut(4)
            .zip(chunk.chunks_exact(4))
        {
            pixel[3] = original[3];
        }
        chunk.copy_from_slice(&output[..chunk.len()]);
    }
    Ok(())
}

#[cfg(windows)]
mod jpeg_preview {
    use super::{apply_icc, output_size, DecodedImage, JpegPreview};
    use image::{metadata::Orientation, DynamicImage, RgbaImage};
    use std::{os::windows::ffi::OsStrExt, path::Path, ptr};
    use windows::core::{w, Interface, PCWSTR, PROPVARIANT};
    use windows::Win32::{
        Foundation::{GENERIC_READ, WINCODEC_ERR_PROPERTYNOTFOUND, WINCODEC_ERR_PROPERTYNOTSUPPORTED},
        Graphics::Imaging::{
            CLSID_WICImagingFactory, CLSID_WICJpegDecoder,
            GUID_WICPixelFormat24bppBGR as BGR24, GUID_WICPixelFormat8bppGray as GRAY8,
            IWICBitmapDecoder, IWICBitmapFrameDecode,
            IWICBitmapSourceTransform, IWICImagingFactory, IWICJpegFrameDecode, IWICStream,
            WICBitmapTransformRotate0, WICColorContextExifColorSpace as EXIF_COLOR_SPACE,
            WICColorContextProfile as ICC_PROFILE,
            WICDecodeMetadataCacheOnDemand, WICJpegFrameHeader, WICJpegScanTypeInterleaved,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED, STATFLAG_NONAME, STATSTG,
        },
    };

    pub(super) const MAX_COMPRESSED_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_SCALED_RGBA_BYTES: usize = 16 * 1024 * 1024;

    struct Apartment;

    impl Drop for Apartment {
        fn drop(&mut self) {
            // Created only after a successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    }

    fn orientation(frame: &IWICBitmapFrameDecode) -> Option<Orientation> {
        let reader = unsafe { frame.GetMetadataQueryReader().ok()? };
        let mut value = PROPVARIANT::default();
        match unsafe { reader.GetMetadataByName(w!("/app1/ifd/{ushort=274}"), &mut value) } {
            Ok(()) => Orientation::from_exif(u8::try_from(u16::try_from(&value).ok()?).ok()?),
            Err(error)
                if matches!(
                    error.code(),
                    WINCODEC_ERR_PROPERTYNOTFOUND | WINCODEC_ERR_PROPERTYNOTSUPPORTED
                ) =>
            {
                Some(Orientation::NoTransforms)
            }
            Err(_) => None,
        }
    }

    fn icc_profile(
        factory: &IWICImagingFactory,
        frame: &IWICBitmapFrameDecode,
    ) -> Option<Option<Vec<u8>>> {
        let mut count = 0;
        // The size query requires NULL, which an empty Rust slice does not provide.
        unsafe {
            (frame.vtable().GetColorContexts)(frame.as_raw(), 0, ptr::null_mut(), &mut count)
                .ok().ok()?;
        }
        if count > 4 {
            return None;
        }
        if count == 0 {
            return Some(None);
        }
        let mut contexts = (0..count)
            .map(|_| unsafe { factory.CreateColorContext().ok().map(Some) })
            .collect::<Option<Vec<_>>>()?;
        unsafe { frame.GetColorContexts(&mut contexts, &mut count).ok()? };
        if count as usize > contexts.len() {
            return None;
        }
        for context in contexts.into_iter().take(count as usize) {
            let context = context?;
            match unsafe { context.GetType().ok()? } {
                EXIF_COLOR_SPACE => continue,
                ICC_PROFILE => {
                    let mut bytes = 0;
                    unsafe {
                        (context.vtable().GetProfileBytes)(
                            context.as_raw(), 0, ptr::null_mut(), &mut bytes,
                        )
                        .ok()
                        .ok()?;
                    }
                    if bytes == 0 || bytes > 4 * 1024 * 1024 {
                        return None;
                    }
                    let mut profile = vec![0; bytes as usize];
                    unsafe { context.GetProfileBytes(&mut profile, &mut bytes).ok()? };
                    return (bytes as usize == profile.len()).then_some(Some(profile));
                }
                _ => return None,
            }
        }
        Some(None)
    }

    pub(super) fn fit(width: u32, height: u32, max_side: u32) -> (u32, u32) {
        let longest = u64::from(width.max(height));
        let side = longest.min(u64::from(max_side));
        let scale = |length| (u64::from(length) * side / longest).max(1) as u32;
        (scale(width), scale(height))
    }

    pub(super) fn decode(
        input: &Path,
        max_side: u32,
        min_pixels: u64,
        max_pixels: u64,
        max_output_bytes: usize,
        managed: bool,
    ) -> Option<JpegPreview> {
        let input_size = super::checked_input_size(input).ok()?;
        if input_size == 0 || input_size > MAX_COMPRESSED_BYTES {
            return None;
        }

        decode_stream(max_side, min_pixels, max_pixels, max_output_bytes, managed, |stream| {
            let path: Vec<u16> = input.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe { stream.InitializeFromFilename(PCWSTR(path.as_ptr()), GENERIC_READ.0).ok()? };
            let mut stat = STATSTG::default();
            unsafe { stream.Stat(&mut stat, STATFLAG_NONAME).ok()? };
            (stat.cbSize == input_size).then_some(())
        })
    }

    pub(super) fn decode_bytes(
        bytes: &[u8],
        max_side: u32,
        max_pixels: u64,
        max_output_bytes: usize,
    ) -> Option<JpegPreview> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_COMPRESSED_BYTES {
            return None;
        }
        // No stream escapes decode_stream, so the borrowed thumbnail remains
        // alive until the read-only JPEG decoder releases every stream reference.
        decode_stream(max_side, 0, max_pixels, max_output_bytes, true, |stream| {
            unsafe { stream.InitializeFromMemory(bytes).ok() }
        })
    }

    fn decode_stream(
        max_side: u32,
        min_pixels: u64,
        max_pixels: u64,
        max_output_bytes: usize,
        managed: bool,
        initialize: impl FnOnce(&IWICStream) -> Option<()>,
    ) -> Option<JpegPreview> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()? };
        let _apartment = Apartment;
        let factory: IWICImagingFactory = unsafe {
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?
        };
        let stream = unsafe { factory.CreateStream().ok()? };
        initialize(&stream)?;
        // Open the specific inbox JPEG decoder. Extension alone never selects a codec.
        let decoder: IWICBitmapDecoder = unsafe {
            CoCreateInstance(&CLSID_WICJpegDecoder, None, CLSCTX_INPROC_SERVER).ok()?
        };
        unsafe { decoder.Initialize(&stream, WICDecodeMetadataCacheOnDemand).ok()? };
        let frame = unsafe { decoder.GetFrame(0).ok()? };
        let mut header = WICJpegFrameHeader::default();
        let jpeg = frame.cast::<IWICJpegFrameDecode>().ok()?;
        unsafe { jpeg.GetFrameHeader(&mut header).ok()? };
        // Multiple scans can require full-image coefficient buffers even when the output is scaled.
        if header.ScanType != WICJpegScanTypeInterleaved || !matches!(header.cComponents, 1 | 3) {
            return None;
        }
        let (source_width, source_height) = (header.Width, header.Height);
        if u64::from(source_width) * u64::from(source_height) < min_pixels {
            return None;
        }
        output_size(source_width, source_height, max_pixels, max_output_bytes).ok()?;
        let transform = frame.cast::<IWICBitmapSourceTransform>().ok()?;
        let (mut width, mut height) = fit(source_width, source_height, max_side);
        unsafe { transform.GetClosestSize(&mut width, &mut height).ok()? };
        if width > source_width
            || height > source_height
            || (width, height) == (source_width, source_height)
        {
            return None;
        }
        let rgba_size = output_size(width, height, max_pixels, MAX_SCALED_RGBA_BYTES).ok()?;
        let mut format = unsafe { frame.GetPixelFormat().ok()? };
        unsafe { transform.GetClosestPixelFormat(&mut format).ok()? };
        let channels = match format {
            BGR24 => 3,
            GRAY8 => 1,
            _ => return None,
        };
        let orientation = orientation(&frame)?;
        let profile = if managed {
            icc_profile(&factory, &frame)?
        } else {
            None
        };
        let mut packed = vec![0; rgba_size / 4 * channels];
        unsafe {
            transform
                .CopyPixels(
                    ptr::null(),
                    width,
                    height,
                    &format,
                    WICBitmapTransformRotate0,
                    width * channels as u32,
                    &mut packed,
                )
                .ok()?;
        }
        let mut pixels = Vec::with_capacity(rgba_size);
        for pixel in packed.chunks_exact(channels) {
            let [r, g, b] = if channels == 3 {
                [pixel[2], pixel[1], pixel[0]]
            } else {
                [pixel[0]; 3]
            };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
        drop(packed);
        let mut image = DynamicImage::ImageRgba8(RgbaImage::from_raw(width, height, pixels)?);
        image.apply_orientation(orientation);
        let (source_width, source_height) = match orientation {
            Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH => (source_height, source_width),
            _ => (source_width, source_height),
        };
        let side = max_side.min(width.max(height));
        let (width, height) = fit(source_width, source_height, side);
        let mut rgba = image.into_rgba8();
        if rgba.dimensions() != (width, height) {
            rgba = image::imageops::thumbnail(&rgba, width, height);
        }
        let mut rgba = rgba.into_raw();
        if let Some(profile) = profile {
            apply_icc(&profile, &mut rgba).ok()?;
        }
        Some(JpegPreview {
            image: DecodedImage {
                width,
                height,
                rgba,
                note: String::new(),
            },
            source_width,
            source_height,
        })
    }
}

#[cfg(not(windows))]
fn decode_raw(_: &Path, _: u64, _: usize, _: bool) -> Result<DecodedImage, String> {
    Err("当前 RAW 引擎仅提供 Windows x64 运行库".into())
}

#[cfg(windows)]
fn decode_raw(
    input: &Path,
    max_pixels: u64,
    max_output_bytes: usize,
    full: bool,
) -> Result<DecodedImage, String> {
    raw::decode(input, max_pixels, max_output_bytes, full)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RawSummary {
    pub width: u32,
    pub height: u32,
    pub note: String,
}

/// Stream genuine LibRaw RGB16 samples to PNG without allocating another full image.
#[cfg(windows)]
pub fn raw16_to_png(
    input: &Path,
    destination: &Path,
    settings: &crate::precision::RawSettings,
    max_bytes: usize,
) -> Result<RawSummary, String> {
    raw::decode16_to_png(input, destination, settings, max_bytes)
}
#[cfg(not(windows))]
pub fn raw16_to_png(
    _input: &Path,
    _destination: &Path,
    _settings: &crate::precision::RawSettings,
    _max_bytes: usize,
) -> Result<RawSummary, String> {
    Err("RAW高位引擎仅提供Windows x64运行库".into())
}

#[cfg(windows)]
mod raw {
    use super::*;
    use libloading::Library;
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    type Handle = *mut c_void;
    type Step = unsafe extern "C" fn(Handle) -> c_int;
    type GetSize = unsafe extern "C" fn(Handle) -> c_int;
    type SetInt = unsafe extern "C" fn(Handle, c_int);
    type MakeImage = unsafe extern "C" fn(Handle, *mut c_int) -> *mut ProcessedImage;
    type ClearImage = unsafe extern "C" fn(*mut ProcessedImage);
    const LIBRAW_IMAGE_JPEG: c_int = 1;
    const LIBRAW_IMAGE_BITMAP: c_int = 2;

    // LibRaw 0.22.x libraw_types.h; the flexible byte array starts at offset 16.
    #[repr(C)]
    struct ProcessedImage {
        kind: c_int,
        height: u16,
        width: u16,
        colors: u16,
        bits: u16,
        data_size: u32,
        data: [u8; 1],
    }

    struct Api {
        init: unsafe extern "C" fn(u32) -> Handle,
        close: unsafe extern "C" fn(Handle),
        open: unsafe extern "C" fn(Handle, *const u16) -> c_int,
        unpack: Step,
        unpack_thumb: Step,
        process: Step,
        make_image: MakeImage,
        make_thumb: MakeImage,
        clear: ClearImage,
        width: GetSize,
        height: GetSize,
        output_bps: SetInt,
        output_color: SetInt,
        no_auto_bright: SetInt,
        demosaic: SetInt,
        highlight: SetInt,
        noise_reduction: SetInt,
        bright: unsafe extern "C" fn(Handle, f32),
        gamma: unsafe extern "C" fn(Handle, c_int, f32),
        cam_mul: unsafe extern "C" fn(Handle, c_int) -> f32,
        user_mul: unsafe extern "C" fn(Handle, c_int, f32),
        strerror: unsafe extern "C" fn(c_int) -> *const c_char,
        version: String,
        _library: Library,
    }

    impl Api {
        fn load() -> Result<Self, String> {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            let candidates = [
                Some(exe_dir.join("native/LibRaw/libraw.dll")),
                Some(exe_dir.join("libraw.dll")),
                cfg!(debug_assertions).then(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("native/LibRaw/libraw.dll")
                }),
            ];
            let path = candidates
                .into_iter()
                .flatten()
                .find(|path| path.is_file())
                .ok_or("缺少 native/LibRaw/libraw.dll（官方 LibRaw 0.22.x x64）")?;
            // The application supplies this official x64 DLL, never a bare DLL search name.
            let library = unsafe { Library::new(&path) }
                .map_err(|error| format!("RAW 引擎加载失败：{error}"))?;
            unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
                library
                    .get::<T>(name)
                    .map(|value| *value)
                    .map_err(|error| format!("RAW C API 不匹配：{error}"))
            }
            unsafe {
                let version: unsafe extern "C" fn() -> c_int =
                    symbol(&library, b"libraw_versionNumber\0")?;
                let version = version();
                if version >> 8 != 22 {
                    return Err("RAW 引擎 ABI 不匹配，需要 LibRaw 0.22.x".into());
                }
                let version_name: unsafe extern "C" fn() -> *const c_char =
                    symbol(&library, b"libraw_version\0")?;
                let version_pointer = version_name();
                let version = if version_pointer.is_null() {
                    "0.22.x".into()
                } else {
                    CStr::from_ptr(version_pointer)
                        .to_string_lossy()
                        .into_owned()
                };
                Ok(Self {
                    init: symbol(&library, b"libraw_init\0")?,
                    close: symbol(&library, b"libraw_close\0")?,
                    open: symbol(&library, b"libraw_open_wfile\0")?,
                    unpack: symbol(&library, b"libraw_unpack\0")?,
                    unpack_thumb: symbol(&library, b"libraw_unpack_thumb\0")?,
                    process: symbol(&library, b"libraw_dcraw_process\0")?,
                    make_image: symbol(&library, b"libraw_dcraw_make_mem_image\0")?,
                    make_thumb: symbol(&library, b"libraw_dcraw_make_mem_thumb\0")?,
                    clear: symbol(&library, b"libraw_dcraw_clear_mem\0")?,
                    width: symbol(&library, b"libraw_get_raw_width\0")?,
                    height: symbol(&library, b"libraw_get_raw_height\0")?,
                    output_bps: symbol(&library, b"libraw_set_output_bps\0")?,
                    output_color: symbol(&library, b"libraw_set_output_color\0")?,
                    no_auto_bright: symbol(&library, b"libraw_set_no_auto_bright\0")?,
                    demosaic: symbol(&library, b"libraw_set_demosaic\0")?,
                    highlight: symbol(&library, b"libraw_set_highlight\0")?,
                    noise_reduction: symbol(&library, b"libraw_set_fbdd_noiserd\0")?,
                    bright: symbol(&library, b"libraw_set_bright\0")?,
                    gamma: symbol(&library, b"libraw_set_gamma\0")?,
                    cam_mul: symbol(&library, b"libraw_get_cam_mul\0")?,
                    user_mul: symbol(&library, b"libraw_set_user_mul\0")?,
                    strerror: symbol(&library, b"libraw_strerror\0")?,
                    version,
                    _library: library,
                })
            }
        }

        fn check(&self, code: c_int, step: &str) -> Result<(), String> {
            if code == 0 {
                return Ok(());
            }
            let message = unsafe {
                let value = (self.strerror)(code);
                if value.is_null() {
                    format!("错误码 {code}")
                } else {
                    CStr::from_ptr(value).to_string_lossy().into_owned()
                }
            };
            Err(format!("{step}：{message}（{code}）"))
        }
    }

    struct Session<'a> {
        api: &'a Api,
        handle: Handle,
    }

    impl<'a> Session<'a> {
        fn open(api: &'a Api, input: &Path) -> Result<Self, String> {
            let handle = unsafe { (api.init)(0) };
            if handle.is_null() {
                return Err("RAW 引擎初始化失败，内存可能不足".into());
            }
            let session = Self { api, handle };
            let path: Vec<u16> = input.as_os_str().encode_wide().chain(Some(0)).collect();
            api.check(unsafe { (api.open)(handle, path.as_ptr()) }, "打开 RAW 失败")?;
            Ok(session)
        }

        fn thumbnail(&self) -> Option<Image<'a>> {
            if unsafe { (self.api.unpack_thumb)(self.handle) } != 0 {
                return None;
            }
            let mut code = 0;
            let pointer = unsafe { (self.api.make_thumb)(self.handle, &mut code) };
            (!pointer.is_null()).then(|| Image { api: self.api, pointer })
        }
    }

    impl Drop for Session<'_> {
        fn drop(&mut self) {
            unsafe { (self.api.close)(self.handle) }
        }
    }

    struct Image<'a> {
        api: &'a Api,
        pointer: *mut ProcessedImage,
    }

    impl Drop for Image<'_> {
        fn drop(&mut self) {
            unsafe { (self.api.clear)(self.pointer) }
        }
    }

    impl Image<'_> {
        fn bytes(&self, limit: usize) -> Result<&[u8], String> {
            let image = unsafe { &*self.pointer };
            let length = image.data_size as usize;
            if length == 0 || length > limit {
                return Err("RAW 结果或预览超过单张内存预算，或没有有效数据".into());
            }
            Ok(unsafe {
                // The producer owns data_size bytes immediately after the fixed C header.
                std::slice::from_raw_parts(
                    std::ptr::addr_of!((*self.pointer).data).cast::<u8>(),
                    length,
                )
            })
        }

        fn decode(&self, max_pixels: u64, max_output_bytes: usize) -> Result<DecodedImage, String> {
            let image = unsafe { &*self.pointer };
            let bytes = self.bytes(max_output_bytes)?;
            match image.kind {
                LIBRAW_IMAGE_JPEG => decode_reader(
                    ImageReader::new(Cursor::new(bytes))
                        .with_guessed_format()
                        .map_err(|error| error.to_string())?,
                    max_pixels,
                    max_output_bytes,
                ),
                LIBRAW_IMAGE_BITMAP => {
                    let width = u32::from(image.width);
                    let height = u32::from(image.height);
                    let output_length = output_size(width, height, max_pixels, max_output_bytes)?;
                    let channels = usize::from(image.colors);
                    if image.bits != 8 || !matches!(channels, 1 | 3) {
                        return Err(format!(
                            "RAW 位图格式暂不支持：{} 位 / {channels} 通道",
                            image.bits
                        ));
                    }
                    let source_length = output_length / 4 * channels;
                    if source_length != bytes.len() {
                        return Err("RAW 位图长度与尺寸不一致".into());
                    }
                    let mut rgba = Vec::new();
                    rgba.try_reserve_exact(output_length)
                        .map_err(|error| format!("RAW 输出内存不足：{error}"))?;
                    for pixel in bytes.chunks_exact(channels) {
                        if channels == 1 {
                            rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]);
                        } else {
                            rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                        }
                    }
                    Ok(DecodedImage {
                        width,
                        height,
                        rgba,
                        note: String::new(),
                    })
                }
                other => Err(format!("RAW 预览编码暂不支持（{other}）")),
            }
        }
    }

    pub fn decode16_to_png(
        input: &Path,
        destination: &Path,
        settings: &crate::precision::RawSettings,
        max_bytes: usize,
    ) -> Result<RawSummary, String> {
        use std::io::Write;
        settings.validate()?;
        if input.metadata().map_err(|e| e.to_string())?.len() > 512 * 1024 * 1024 {
            return Err("RAW输入超过512 MiB".into());
        }
        let api = Api::load()?;
        let handle = unsafe { (api.init)(0) };
        if handle.is_null() {
            return Err("RAW引擎初始化失败".into());
        }
        let session = Session { api: &api, handle };
        let path: Vec<u16> = input.as_os_str().encode_wide().chain(Some(0)).collect();
        api.check(
            unsafe { (api.open)(session.handle, path.as_ptr()) },
            "打开RAW失败",
        )?;
        let (width, height) =
            unsafe { ((api.width)(session.handle), (api.height)(session.handle)) };
        if width <= 0
            || height <= 0
            || (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(6))
                .is_none_or(|n| n > max_bytes)
        {
            return Err("RAW高位输出超过单幅预算".into());
        }
        unsafe {
            (api.output_bps)(session.handle, 16);
            (api.output_color)(session.handle, settings.output_color as c_int);
            (api.no_auto_bright)(session.handle, (!settings.auto_bright) as c_int);
            (api.bright)(session.handle, settings.brightness);
            (api.demosaic)(session.handle, settings.demosaic as c_int);
            (api.highlight)(session.handle, settings.highlight as c_int);
            (api.noise_reduction)(session.handle, settings.noise_reduction as c_int);
            for (index, value) in settings.gamma.iter().enumerate() {
                (api.gamma)(session.handle, index as c_int, *value);
            }
            for index in 0..4 {
                let value = settings
                    .white_balance
                    .map(|v| v[index])
                    .unwrap_or_else(|| (api.cam_mul)(session.handle, index as c_int));
                if value.is_finite() && value > 0.0 {
                    (api.user_mul)(session.handle, index as c_int, value);
                }
            }
        }
        api.check(unsafe { (api.unpack)(session.handle) }, "解包RAW失败")?;
        api.check(unsafe { (api.process)(session.handle) }, "RAW16显影失败")?;
        let mut code = 0;
        let pointer = unsafe { (api.make_image)(session.handle, &mut code) };
        if pointer.is_null() {
            api.check(code, "读取RAW16显影失败")?;
            return Err("RAW16没有返回图像".into());
        }
        let image = Image { api: &api, pointer };
        let header = unsafe { &*image.pointer };
        if header.kind != LIBRAW_IMAGE_BITMAP
            || header.bits != 16
            || !matches!(header.colors, 1 | 3)
        {
            return Err("RAW16引擎返回的样本类型不受支持".into());
        }
        let (width, height) = (header.width as u32, header.height as u32);
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(header.colors as usize * 2))
            .filter(|n| *n <= max_bytes)
            .ok_or("RAW16尺寸溢出或超过预算")?;
        if expected != header.data_size as usize {
            return Err("RAW16返回长度与尺寸不符".into());
        }
        let source = unsafe {
            std::slice::from_raw_parts(
                std::ptr::addr_of!((*image.pointer).data).cast::<u8>(),
                expected,
            )
        };
        let file = std::fs::File::create(destination).map_err(|e| e.to_string())?;
        let mut info = png::Info::with_size(width, height);
        info.color_type = if header.colors == 1 {
            png::ColorType::Grayscale
        } else {
            png::ColorType::Rgb
        };
        info.bit_depth = png::BitDepth::Sixteen;
        if header.colors == 3 {
            info.icc_profile = Some(std::borrow::Cow::Owned(crate::precision::raw_profile(
                settings, false,
            )?));
        }
        let mut encoder = png::Encoder::with_info(std::io::BufWriter::new(file), info)
            .map_err(|e| e.to_string())?;
        // This is an intermediate, bounded stream; avoid spending the worker timeout on DEFLATE.
        encoder.set_compression(png::Compression::Fastest);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        let mut stream = writer
            .stream_writer_with_size(64 * 1024)
            .map_err(|e| e.to_string())?;
        let mut converted = vec![0u8; 64 * 1024];
        for chunk in source.chunks(64 * 1024) {
            for (pair, target) in chunk.chunks_exact(2).zip(converted.chunks_exact_mut(2)) {
                target.copy_from_slice(&u16::from_ne_bytes([pair[0], pair[1]]).to_be_bytes());
            }
            stream
                .write_all(&converted[..chunk.len()])
                .map_err(|e| e.to_string())?;
        }
        stream.finish().map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        Ok(RawSummary {
            width,
            height,
            note: format!("LibRaw {} 原始数据显影 · 真实16位样本", api.version),
        })
    }

    pub(super) fn decode_preview(
        input: &Path,
        max_side: u32,
        max_pixels: u64,
        max_output_bytes: usize,
    ) -> Option<JpegPreview> {
        let api = Api::load().ok()?;
        let session = Session::open(&api, input).ok()?;
        let thumbnail = session.thumbnail()?;
        if unsafe { (*thumbnail.pointer).kind } != LIBRAW_IMAGE_JPEG {
            return None;
        }
        let bytes = thumbnail
            .bytes((jpeg_preview::MAX_COMPRESSED_BYTES as usize).min(max_output_bytes))
            .ok()?;
        let note = "RAW 内嵌 JPEG 预览";
        if let Some(mut preview) =
            jpeg_preview::decode_bytes(bytes, max_side, max_pixels, max_output_bytes)
        {
            preview.image.note = note.into();
            return Some(preview);
        }

        // Small or unsupported scaled JPEGs can still use the existing JPEG
        // decoder. Bound its decoded buffer separately from the full RAW path.
        const MAX_FALLBACK_RGBA_BYTES: usize = 16 * 1024 * 1024;
        let image = decode_reader(
            ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg),
            max_pixels,
            max_output_bytes.min(MAX_FALLBACK_RGBA_BYTES),
        )
        .ok()?;
        let (source_width, source_height) = (image.width, image.height);
        let (width, height) = jpeg_preview::fit(source_width, source_height, max_side);
        let rgba = if (width, height) == (source_width, source_height) {
            image.rgba
        } else {
            let image = image::RgbaImage::from_raw(source_width, source_height, image.rgba)?;
            image::imageops::thumbnail(&image, width, height).into_raw()
        };
        Some(JpegPreview {
            image: DecodedImage {
                width,
                height,
                rgba,
                note: note.into(),
            },
            source_width,
            source_height,
        })
    }

    pub fn decode(
        input: &Path,
        max_pixels: u64,
        max_output_bytes: usize,
        full: bool,
    ) -> Result<DecodedImage, String> {
        let api = Api::load()?;
        let session = Session::open(&api, input)?;
        let mut bitmap_preview_without_orientation = false;
        if let Some(image) = (!full).then(|| session.thumbnail()).flatten() {
            // This C API returns no orientation for bitmap thumbnails; full rendering applies it.
            if unsafe { (*image.pointer).kind } == LIBRAW_IMAGE_BITMAP {
                bitmap_preview_without_orientation = true;
            } else if let Ok(mut decoded) = image.decode(max_pixels, max_output_bytes) {
                decoded.note = format!("RAW 内嵌预览（可切换完整解码） · {}", decoded.note);
                return Ok(decoded);
            }
        }
        let width = unsafe { (api.width)(session.handle) };
        let height = unsafe { (api.height)(session.handle) };
        if width <= 0 || height <= 0 {
            return Err("RAW 未提供有效的原始尺寸".into());
        }
        output_size(width as u32, height as u32, max_pixels, max_output_bytes)?;
        unsafe {
            (api.output_bps)(session.handle, 8);
            (api.output_color)(session.handle, 1); // LibRaw's sRGB output.
            for index in 0..4 {
                let multiplier = (api.cam_mul)(session.handle, index);
                if multiplier.is_finite() && multiplier > 0.0 {
                    (api.user_mul)(session.handle, index, multiplier);
                }
            }
        }
        api.check(unsafe { (api.unpack)(session.handle) }, "解包 RAW 失败")?;
        api.check(unsafe { (api.process)(session.handle) }, "RAW 显影失败")?;
        let mut code = 0;
        let pointer = unsafe { (api.make_image)(session.handle, &mut code) };
        if pointer.is_null() {
            api.check(code, "读取 RAW 显影结果失败")?;
            return Err("RAW 显影没有返回图像".into());
        }
        let image = Image { api: &api, pointer };
        let mut decoded = image.decode(max_pixels, max_output_bytes)?;
        decoded.note = if full {
            format!("LibRaw {} 完整解码 · sRGB · 8 位显示", api.version)
        } else if bitmap_preview_without_orientation {
            format!(
                "位图预览缺少方向信息，已用 LibRaw {} 完整解码 · sRGB",
                api.version
            )
        } else {
            format!(
                "无可用内嵌预览，已用 LibRaw {} 完整解码 · sRGB",
                api.version
            )
        };
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_png_preserves_pixels_and_releases_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("中文测试.png");
        let pixels = vec![255, 0, 0, 255, 0, 128, 255, 64];
        image::save_buffer(&path, &pixels, 2, 1, image::ExtendedColorType::Rgba8).unwrap();
        let decoded = decode(&path, 2, 8, false).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.rgba, pixels);
        std::fs::rename(&path, directory.path().join("moved.png")).unwrap();
    }

    #[test]
    fn rejects_pixel_and_output_limits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("limit.png");
        image::save_buffer(&path, &[0; 24], 3, 2, image::ExtendedColorType::Rgba8).unwrap();
        assert!(decode(&path, 5, 100, false).is_err());
        assert!(decode(&path, 6, 23, false).is_err());
        assert!(output_size(u32::MAX, u32::MAX, u64::MAX, usize::MAX).is_err());
    }

    #[test]
    fn webp_is_decoded_without_loss() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.webp");
        let pixels = vec![25, 120, 230, 255, 230, 40, 15, 100];
        image::save_buffer(&path, &pixels, 2, 1, image::ExtendedColorType::Rgba8).unwrap();
        let decoded = decode(&path, 2, 8, false).unwrap();
        assert_eq!(decoded.rgba, pixels);
    }

    #[test]
    fn tga_uses_extension_fallback_but_magic_has_priority() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.tga");
        let pixels = [255_u8, 20, 30, 255, 10, 180, 240, 80];
        image::save_buffer_with_format(
            &path,
            &pixels,
            2,
            1,
            image::ExtendedColorType::Rgba8,
            ImageFormat::Tga,
        )
        .unwrap();
        assert_eq!(decode(&path, 2, 128, false).unwrap().rgba, pixels);
        image::save_buffer_with_format(
            &path,
            &pixels,
            2,
            1,
            image::ExtendedColorType::Rgba8,
            ImageFormat::Png,
        )
        .unwrap();
        assert_eq!(decode(&path, 2, 128, false).unwrap().rgba, pixels);
    }

    #[test]
    fn icc_conversion_keeps_alpha_and_srgb_values() {
        let profile = ColorProfile::new_srgb().encode().unwrap();
        let original = [20_u8, 30, 40, 0, 80, 160, 240, 73, 255, 255, 255, 255];
        let mut pixels = original;
        apply_icc(&profile, &mut pixels).unwrap();
        for (before, after) in original.chunks_exact(4).zip(pixels.chunks_exact(4)) {
            assert_eq!(before[3], after[3]);
            for channel in 0..3 {
                assert!(before[channel].abs_diff(after[channel]) <= 2);
            }
        }
    }

    #[test]
    fn jpeg_exif_orientation_is_applied() {
        let directory = tempfile::tempdir().unwrap();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[128; 18], 2, 3, image::ExtendedColorType::Rgb8)
            .unwrap();
        let exif = [
            b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0,
            0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ];
        let length = (exif.len() + 2) as u16;
        let mut file = vec![0xff, 0xd8, 0xff, 0xe1];
        file.extend_from_slice(&length.to_be_bytes());
        file.extend_from_slice(&exif);
        file.extend_from_slice(&jpeg[2..]);
        let path = directory.path().join("rotated.jpg");
        std::fs::write(&path, file).unwrap();
        let decoded = decode(&path, 6, 128, false).unwrap();
        assert_eq!((decoded.width, decoded.height), (3, 2));
    }

    #[cfg(windows)]
    #[test]
    fn jpeg_scaled_preview_uses_native_scale_and_checks_source_limits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preview.JPG");
        let (width, height) = (1281, 641);
        let rgb = [24, 140, 218].repeat(width * height);
        image::save_buffer_with_format(&path, &rgb, width as u32, height as u32,
            image::ExtendedColorType::Rgb8, ImageFormat::Jpeg).unwrap();
        let preview = decode_jpeg_preview(&path, 64, 0, 2_000_000, 8 * 1024 * 1024, true).unwrap();
        assert_eq!((preview.source_width, preview.source_height), (1281, 641));
        assert_eq!((preview.image.width, preview.image.height), (64, 32));
        assert_eq!(preview.image.rgba.len(), 64 * 32 * 4);
        assert!(preview.image.rgba.chunks_exact(4).all(|pixel| {
            pixel[3] == 255 && pixel[..3].iter().zip([24, 140, 218])
                .all(|(&actual, expected)| actual.abs_diff(expected) <= 4)
        }));
        assert!(decode_jpeg_preview(&path, 64, 0, 2_000_000, 1000, true).is_none());
        assert!(decode_jpeg_preview(&path, 64, 0, 1000, 8 * 1024 * 1024, true).is_none());
        assert!(decode_jpeg_preview(&path, 2048, 0, 2_000_000, 8 * 1024 * 1024, true).is_none());
        assert!(decode_jpeg_preview(&path, 64, 2_000_000, 2_000_000, 8 * 1024 * 1024, true).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn jpeg_scaled_preview_applies_exif_and_icc_like_full_decode() {
        use image::ImageEncoder;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("旋转色彩.jpg");
        let (width, height) = (1280, 640);
        let mut rgb = [176, 80, 145].repeat(width * height / 2);
        rgb.extend([64, 174, 99].repeat(width * height / 2));
        let exif = [
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0,
            6, 0, 0, 0, 0, 0, 0, 0,
        ];
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95);
        encoder.set_exif_metadata(exif.to_vec()).unwrap();
        encoder.set_icc_profile(ColorProfile::new_adobe_rgb().encode().unwrap()).unwrap();
        encoder.encode(&rgb, width as u32, height as u32, image::ExtendedColorType::Rgb8).unwrap();
        std::fs::write(&path, jpeg).unwrap();

        for managed in [true, false] {
            let full = decode_options(&path, 2_000_000, 8 * 1024 * 1024, false, managed).unwrap();
            let preview = decode_jpeg_preview(&path, 128, 0, 2_000_000, 8 * 1024 * 1024, managed).unwrap();
            assert_eq!((preview.source_width, preview.source_height), (640, 1280));
            assert_eq!((preview.image.width, preview.image.height), (64, 128));
            for x in [8, 56] {
                let scaled = ((64 * 64 + x) * 4) as usize;
                let source = ((640 * 640 + x * 10) * 4) as usize;
                for channel in 0..4 {
                    assert!(preview.image.rgba[scaled + channel].abs_diff(full.rgba[source + channel]) <= 5);
                }
            }
        }
    }

    #[test]
    fn supported_extensions_are_case_insensitive() {
        assert!(is_supported_image("subfolder/PHOTO.WEBP"));
        assert!(is_supported_image("design/PHOTO.PSD"));
        assert!(is_supported_image("design/PHOTO.PSB"));
        assert!(is_raw("camera.CR3"));
        assert!(!is_supported_image("movie.mp4"));
        assert!(!is_supported_image("archive.zip"));
    }

    #[test]
    fn large_input_allowance_requires_photoshop_name_and_file_header() {
        use std::io::Write;
        assert_eq!(input_byte_limit("document.PSD"), 64 * 1024 * 1024 * 1024);
        assert_eq!(input_byte_limit("document.psb"), 64 * 1024 * 1024 * 1024);
        for name in ["photo.png", "photo.jpg", "photo.nef", "photo.webp"] {
            assert_eq!(input_byte_limit(name), 512 * 1024 * 1024);
        }
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("renamed.psd");
        let mut file = File::create(&input).unwrap();
        file.write_all(b"\x89PNG\r\n").unwrap();
        file.set_len(MAX_INPUT_BYTES + 1).unwrap();
        drop(file);
        assert!(checked_input_size(&input)
            .unwrap_err()
            .contains("不能仅改文件后缀"));
        std::fs::rename(&input, directory.path().join("renamed.png")).unwrap();
        assert!(checked_input_size(&directory.path().join("renamed.png"))
            .unwrap_err()
            .contains("超过512 MiB"));
        assert!(checked_input_size(directory.path())
            .unwrap_err()
            .contains("普通文件"));
    }

    #[cfg(windows)]
    #[test]
    fn libraw_decodes_synthetic_dng_and_rejects_oversized_raw() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("合成测试.dng");
        std::fs::write(
            &path,
            include_bytes!("../native/LibRaw/testdata/synthetic.dng"),
        )
        .unwrap();
        assert!(decode_raw_preview(&path, 64, 1000, 8192).is_none());
        for full in [false, true] {
            let decoded = decode(&path, 1000, 8192, full).unwrap();
            assert_eq!((decoded.width, decoded.height), (32, 24));
            assert_eq!(decoded.rgba.len(), 32 * 24 * 4);
            assert!(decoded.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        }
        let error = decode(&path, 100, 8192, true).err().unwrap();
        assert!(error.contains("像素限制"), "{error}");
        std::fs::rename(&path, directory.path().join("moved.dng")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the optional public camera samples"]
    fn raw_embedded_preview_matches_normal_orientation_and_geometry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-output/modern-raw-samples");
        for name in [
            "Nikon-Z6.NEF",
            "Sony-a7III.ARW",
            "Canon-EOS-R.CR3",
            "Apple-iPhone-12-Pro.DNG",
        ] {
            let path = root.join(name);
            let normal = decode(&path, 32_000_000, 128 * 1024 * 1024, false).unwrap();
            let thumbnail = decode_raw_preview(&path, 128, 32_000_000, 128 * 1024 * 1024)
                .unwrap_or_else(|| panic!("embedded JPEG preview missing: {name}"));
            assert_eq!(
                (thumbnail.source_width, thumbnail.source_height),
                (normal.width, normal.height),
                "{name}"
            );
            let (width, height) = (thumbnail.image.width, thumbnail.image.height);
            assert!(width <= 128 && height <= 128, "{name}");
            assert_eq!(thumbnail.image.rgba.len(), (width * height * 4) as usize, "{name}");
        }
    }
}
