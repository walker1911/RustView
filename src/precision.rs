//! Bounded, high-depth editing through the bundled ImageMagick Q16-HDRI engine.
use crate::editing::{self, ColorDepth, Edit, EditedImage};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const MAX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;
// Recipes permit 128 user steps; a worker preview may add a final RGBA8 conversion.
const MAX_EDITS: usize = 129;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    Png,
    Jpeg,
    Tiff,
    Bmp,
    WebP,
    Gif,
    Avif,
}
impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tif",
            Self::Bmp => "bmp",
            Self::WebP => "webp",
            Self::Gif => "gif",
            Self::Avif => "avif",
        }
    }
    fn coder(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Tiff => "TIFF",
            Self::Bmp => "BMP",
            Self::WebP => "WEBP",
            Self::Gif => "GIF",
            Self::Avif => "AVIF",
        }
    }
    pub fn from_path(path: &Path) -> Result<Self, String> {
        Ok(
            match path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "png" => Self::Png,
                "jpg" | "jpeg" => Self::Jpeg,
                "tif" | "tiff" => Self::Tiff,
                "bmp" => Self::Bmp,
                "webp" => Self::WebP,
                "gif" => Self::Gif,
                "avif" => Self::Avif,
                _ => return Err("输出请选择PNG/JPEG/TIFF/BMP/WebP/GIF/AVIF".into()),
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationMode {
    CurrentFrame,
    AllFrames,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub format: OutputFormat,
    pub quality: u8,
    /// 8 or 16. JPEG/WebP/GIF/BMP are encoded at their supported precision.
    pub depth: u8,
    pub preserve_metadata: bool,
    pub animation: AnimationMode,
}
impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: OutputFormat::Png,
            quality: 95,
            depth: 16,
            preserve_metadata: true,
            animation: AnimationMode::CurrentFrame,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RawSettings {
    /// None uses the camera's recorded white balance; explicit values are sensor channel multipliers.
    pub white_balance: Option<[f32; 4]>,
    pub exposure_ev: f32,
    pub brightness: f32,
    pub auto_bright: bool,
    /// LibRaw output color: 1=sRGB, 2=Adobe RGB, 4=ProPhoto RGB.
    pub output_color: u8,
    pub gamma: [f32; 2],
    pub demosaic: u8,
    pub highlight: u8,
    pub noise_reduction: u8,
}
impl Default for RawSettings {
    fn default() -> Self {
        Self {
            white_balance: None,
            exposure_ev: 0.0,
            brightness: 1.0,
            auto_bright: true,
            output_color: 1,
            gamma: [1.0 / 2.222, 4.5],
            demosaic: 3,
            highlight: 0,
            noise_reduction: 0,
        }
    }
}
impl RawSettings {
    pub fn validate(&self) -> Result<(), String> {
        finite(self.exposure_ev, -10.0, 10.0, "RAW曝光")?;
        finite(self.brightness, 0.01, 100.0, "RAW亮度")?;
        if !matches!(self.output_color, 1 | 2 | 4)
            || !matches!(self.demosaic, 0 | 1 | 2 | 3 | 4 | 11 | 12)
            || self.highlight > 9
            || self.noise_reduction > 2
        {
            return Err("RAW颜色空间、去马赛克或高光/降噪选项无效".into());
        }
        finite(self.gamma[0], 0.05, 1.0, "RAW Gamma指数")?;
        finite(self.gamma[1], 1.0, 100.0, "RAW Gamma线性斜率")?;
        if self.gamma[0] == 1.0 && self.gamma[1] != 1.0 {
            return Err("线性RAW Gamma必须使用[1,1]".into());
        }
        if let Some(values) = self.white_balance {
            for value in values {
                finite(value, 0.01, 100.0, "RAW白平衡")?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderSummary {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub depth: u8,
    pub note: String,
}

fn finite(value: f32, min: f32, max: f32, name: &str) -> Result<(), String> {
    if !value.is_finite() || value < min || value > max {
        Err(format!("{name}必须在{min}–{max}之间"))
    } else {
        Ok(())
    }
}

/// The ICC inverse transfer curve matches LibRaw's continuous power/toe output curve.
pub(crate) fn raw_profile(settings: &RawSettings, linear: bool) -> Result<Vec<u8>, String> {
    use moxcms::{ColorProfile, ToneReprCurve};
    let mut profile = match settings.output_color {
        2 => ColorProfile::new_adobe_rgb(),
        4 => ColorProfile::new_pro_photo_rgb(),
        _ => ColorProfile::new_srgb(),
    };
    let (power, slope) = (settings.gamma[0] as f64, settings.gamma[1] as f64);
    let curve = if linear {
        moxcms::curve_from_gamma(1.0)
    } else if slope == 1.0 || power == 1.0 {
        moxcms::curve_from_gamma((1.0 / power) as f32)
    } else {
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for _ in 0..80 {
            let t = (lo + hi) / 2.0;
            let f = 1.0 + slope * t * (1.0 / power - 1.0) - slope * t.powf(1.0 - power) / power;
            if f > 0.0 {
                lo = t
            } else {
                hi = t
            }
        }
        let toe = (lo + hi) / 2.0;
        let offset = slope * toe * (1.0 / power - 1.0);
        ToneReprCurve::Parametric(vec![
            (1.0 / power) as f32,
            (1.0 / (1.0 + offset)) as f32,
            (offset / (1.0 + offset)) as f32,
            (1.0 / slope) as f32,
            (slope * toe) as f32,
            0.0,
            0.0,
        ])
    };
    profile.red_trc = Some(curve.clone());
    profile.green_trc = Some(curve.clone());
    profile.blue_trc = Some(curve);
    profile.cicp = None; // The custom LibRaw transfer curve is not the sRGB CICP transfer.
    encode_profile(&profile)
}

fn encode_profile(profile: &moxcms::ColorProfile) -> Result<Vec<u8>, String> {
    let encoded = profile.encode().map_err(|e| e.to_string())?;
    let number =
        |offset| u32::from_be_bytes(encoded[offset..offset + 4].try_into().unwrap()) as usize;
    let count = number(128);
    let table_end = 132 + count * 12;
    if table_end > encoded.len() {
        return Err("ICC标签表超出范围".into());
    }
    let mut bytes = encoded[..table_end].to_vec();
    // moxcms 0.7 emits some unaligned string/curve tags. Repack tag payloads,
    // preserving their contents; ICC PCS/display media white are exact D50.
    let d50 = [0x0000f6d6_u32, 0x00010000, 0x0000d32d];
    for (index, value) in d50.iter().enumerate() {
        bytes[68 + index * 4..72 + index * 4].copy_from_slice(&value.to_be_bytes());
    }
    // Deterministic profile bytes avoid a needless identity color transform.
    for (index, value) in [2000u16, 1, 1, 0, 0, 0].iter().enumerate() {
        bytes[24 + index * 2..26 + index * 2].copy_from_slice(&value.to_be_bytes());
    }
    for index in 0..count {
        let entry = 132 + index * 12;
        let (start, length) = (number(entry + 4), number(entry + 8));
        let payload = encoded
            .get(start..start.checked_add(length).ok_or("ICC标签长度溢出")?)
            .ok_or("ICC标签数据超出范围")?;
        bytes.resize((bytes.len() + 3) & !3, 0);
        let offset = bytes.len();
        bytes.extend_from_slice(payload);
        if &bytes[entry..entry + 4] == b"wtpt" && length >= 20 {
            for (index, value) in d50.iter().enumerate() {
                bytes[offset + 8 + index * 4..offset + 12 + index * 4]
                    .copy_from_slice(&value.to_be_bytes());
            }
        }
        bytes[entry + 4..entry + 8].copy_from_slice(&(offset as u32).to_be_bytes());
    }
    bytes.resize((bytes.len() + 3) & !3, 0);
    let length = bytes.len() as u32;
    bytes[..4].copy_from_slice(&length.to_be_bytes());
    Ok(bytes)
}

pub fn engine_dir() -> Result<PathBuf, String> {
    let directory = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("native/ImageMagick")));
    let ancestors = std::env::current_exe().ok().into_iter().flat_map(|p| {
        p.ancestors()
            .take(4)
            .map(|p| p.join("native/ImageMagick"))
            .collect::<Vec<_>>()
    });
    directory
        .into_iter()
        .chain(ancestors)
        .chain(
            cfg!(debug_assertions)
                .then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("native/ImageMagick")),
        )
        .find(|p| p.join("magick.exe").is_file())
        .ok_or_else(|| "缺少native/ImageMagick/magick.exe（官方Q16-HDRI x64图像引擎）".into())
}

fn run(arguments: &[OsString], work: &Path) -> Result<(), String> {
    run_command(arguments, work, None, None)
}
fn run_command(
    arguments: &[OsString],
    work: &Path,
    mode: Option<&str>,
    stdout: Option<&Path>,
) -> Result<(), String> {
    let engine = engine_dir()?;
    let mut command = Command::new(engine.join("magick.exe"));
    let limits = [
        "-limit",
        "memory",
        "128MiB",
        "-limit",
        "map",
        "0",
        "-limit",
        "disk",
        "1GiB",
        "-limit",
        "thread",
        "4",
        "-limit",
        "time",
        "120",
        "-define",
        "filename:literal=true",
    ];
    let use_script = mode.is_none()
        && arguments
            .iter()
            .map(|a| a.to_string_lossy().encode_utf16().count() + 3)
            .sum::<usize>()
            > 24000;
    if use_script {
        let mut script = String::new();
        for argument in limits.iter().map(|a| OsString::from(*a)).chain(
            arguments
                .iter()
                .take(arguments.len().saturating_sub(1))
                .cloned(),
        ) {
            script.push_str(&quote_script_argument(&argument.to_string_lossy()));
            script.push('\n');
        }
        script.push_str("-write\n");
        script.push_str(&quote_script_argument(
            &arguments.last().ok_or("缺少输出文件")?.to_string_lossy(),
        ));
        script.push('\n');
        let path = work.join("render.magick");
        fs::write(&path, script).map_err(|e| e.to_string())?;
        command.arg("-script").arg(path);
    } else {
        if let Some(mode) = mode {
            command.arg(mode);
        }
        command.args(limits).args(arguments);
    }
    command
        .current_dir(work)
        .env("MAGICK_CONFIGURE_PATH", &engine)
        .env("MAGICK_TEMPORARY_PATH", work)
        .env("MAGICK_OCL_DEVICE", "off")
        .stdin(Stdio::null())
        .stdout(match stdout {
            Some(path) => Stdio::from(fs::File::create(path).map_err(|e| e.to_string())?),
            None => Stdio::null(),
        })
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动高位图像引擎：{e}"))?;
    let error_reader = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let keep = count.min((32 * 1024usize).saturating_sub(text.len()));
                        text.extend_from_slice(&chunk[..keep]);
                    }
                }
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    });
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("图像编辑超过120秒，已停止；原文件未改变".into());
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                let message = error_reader
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default();
                if exit.success() {
                    return Ok(());
                }
                return Err(format!(
                    "图像操作未完成（可能超出128 MiB内存缓存／1 GiB磁盘缓存限额）：{}",
                    message.trim()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    }
}

fn quote_script_argument(argument: &str) -> String {
    format!(
        "\"{}\"",
        argument
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

fn push(arguments: &mut Vec<OsString>, values: impl IntoIterator<Item = impl Into<OsString>>) {
    arguments.extend(values.into_iter().map(Into::into));
}
fn color(value: [u8; 4]) -> String {
    format!(
        "rgba({},{},{},{:.6})",
        value[0],
        value[1],
        value[2],
        value[3] as f32 / 255.0
    )
}
fn literal_text(value: &str) -> Result<String, String> {
    if value.len() > 16 * 1024 || value.contains('\0') {
        return Err("文字水印过长或包含空字符".into());
    }
    let value = value.replace('\\', "\\\\").replace('%', "%%");
    Ok(if value.starts_with('@') {
        format!("\\{value}")
    } else {
        value
    })
}

pub fn is_native(edit: &Edit) -> bool {
    !matches!(
        edit,
        Edit::Resize { .. }
            | Edit::Exposure { .. }
            | Edit::Rotate90 { .. }
            | Edit::FlipH
            | Edit::FlipV
            | Edit::Grayscale
            | Edit::ColorDepth { .. }
            | Edit::Crop { .. }
    )
}

fn add_edit(
    arguments: &mut Vec<OsString>,
    edit: &Edit,
    work: &Path,
    index: usize,
) -> Result<(), String> {
    match edit {
        Edit::Resize { width, height } => {
            if *width == 0
                || *height == 0
                || (*width as u64) * (*height as u64) * 8 > MAX_OUTPUT_BYTES
            {
                return Err("调整尺寸超过高位图像512 MiB预算".into());
            }
            push(
                arguments,
                [
                    "-filter".into(),
                    "Lanczos".into(),
                    "-resize".into(),
                    format!("{width}x{height}!"),
                ],
            );
        }
        Edit::Exposure { ev } => {
            finite(*ev, -10.0, 10.0, "曝光")?;
            push(
                arguments,
                [
                    "-profile".into(),
                    work.join("linear.icc").into_os_string(),
                    "-channel".into(),
                    "RGB".into(),
                    "-evaluate".into(),
                    "Multiply".into(),
                    2f64.powf(*ev as f64).to_string().into(),
                    "+channel".into(),
                    "-profile".into(),
                    work.join("working.icc").into_os_string(),
                ],
            );
        }
        Edit::Rotate90 { turns } => push(
            arguments,
            ["-rotate".into(), ((turns % 4) as u32 * 90).to_string()],
        ),
        Edit::FlipH => push(arguments, ["-flop"]),
        Edit::FlipV => push(arguments, ["-flip"]),
        Edit::Grayscale => push(arguments, ["-colorspace", "Gray"]),
        Edit::Crop {
            x,
            y,
            width,
            height,
        } => {
            if *width == 0 || *height == 0 {
                return Err("裁剪宽高无效".into());
            }
            push(
                arguments,
                [
                    "-crop".into(),
                    format!("{width}x{height}+{x}+{y}"),
                    "+repage".into(),
                ],
            );
        }
        Edit::ColorDepth { mode } => match mode {
            ColorDepth::Rgba8 => push(arguments, ["-depth", "8"]),
            ColorDepth::BlackWhite1 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-colorspace",
                    "Gray",
                    "-threshold",
                    "50%",
                    "-depth",
                    "1",
                ],
            ),
            ColorDepth::Gray4 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-colorspace",
                    "Gray",
                    "-posterize",
                    "16",
                    "-depth",
                    "4",
                ],
            ),
            ColorDepth::Gray8 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-colorspace",
                    "Gray",
                    "-depth",
                    "8",
                ],
            ),
            ColorDepth::Indexed4 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-colors",
                    "16",
                    "-type",
                    "Palette",
                    "-depth",
                    "4",
                ],
            ),
            ColorDepth::Indexed8 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-colors",
                    "256",
                    "-type",
                    "Palette",
                    "-depth",
                    "8",
                ],
            ),
            ColorDepth::Rgb15 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-posterize",
                    "32",
                    "-depth",
                    "5",
                ],
            ),
            ColorDepth::Rgb24 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-type",
                    "TrueColor",
                    "-depth",
                    "8",
                ],
            ),
            ColorDepth::Gray16 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-colorspace",
                    "Gray",
                    "-depth",
                    "16",
                ],
            ),
            ColorDepth::Rgb48 => push(
                arguments,
                [
                    "-background",
                    "white",
                    "-alpha",
                    "remove",
                    "-type",
                    "TrueColor",
                    "-depth",
                    "16",
                ],
            ),
        },
        Edit::WhiteBalance { red, green, blue } => {
            for v in [red, green, blue] {
                finite(*v, 0.01, 10.0, "白平衡通道")?;
            }
            push(
                arguments,
                [
                    "-profile".into(),
                    work.join("linear.icc").into_os_string(),
                    "-color-matrix".into(),
                    format!("{red},0,0,0,{green},0,0,0,{blue}").into(),
                    "-profile".into(),
                    work.join("working.icc").into_os_string(),
                ],
            );
        }
        Edit::Tone {
            brightness,
            contrast,
            gamma,
        } => {
            finite(*brightness, -100.0, 100.0, "亮度")?;
            finite(*contrast, -100.0, 100.0, "对比")?;
            finite(*gamma, 0.1, 10.0, "Gamma")?;
            push(
                arguments,
                [
                    "-brightness-contrast".into(),
                    format!("{brightness}x{contrast}"),
                    "-gamma".into(),
                    gamma.to_string(),
                ],
            );
        }
        Edit::Levels {
            black,
            white,
            gamma,
        } => {
            finite(*black, 0.0, 99.9, "黑场")?;
            finite(*white, 0.1, 100.0, "白场")?;
            finite(*gamma, 0.1, 10.0, "Gamma")?;
            if black >= white {
                return Err("黑场必须小于白场".into());
            }
            push(
                arguments,
                ["-level".into(), format!("{black}%,{white}%,{gamma}")],
            );
        }
        Edit::Saturation { percent } => {
            finite(*percent, 0.0, 300.0, "饱和度")?;
            push(
                arguments,
                ["-modulate".into(), format!("100,{percent},100")],
            );
        }
        Edit::Hue { degrees } => {
            finite(*degrees, -180.0, 180.0, "色相")?;
            push(
                arguments,
                [
                    "-modulate".into(),
                    format!("100,100,{}", 100.0 + degrees / 1.8),
                ],
            );
        }
        Edit::ChannelMixer { matrix } => {
            for value in matrix {
                finite(*value, -4.0, 4.0, "通道混合")?;
            }
            push(
                arguments,
                [
                    "-color-matrix".into(),
                    matrix
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                ],
            );
        }
        Edit::AutoLevels => push(arguments, ["-auto-level"]),
        Edit::AutoContrast => push(arguments, ["-contrast-stretch", "0.1%x0.1%"]),
        Edit::Sharpen {
            sigma,
            amount,
            threshold,
        } => {
            finite(*sigma, 0.1, 20.0, "锐化半径")?;
            finite(*amount, 0.0, 5.0, "锐化强度")?;
            finite(*threshold, 0.0, 1.0, "锐化阈值")?;
            push(
                arguments,
                ["-unsharp".into(), format!("0x{sigma}+{amount}+{threshold}")],
            );
        }
        Edit::Blur { sigma } => {
            finite(*sigma, 0.1, 100.0, "模糊半径")?;
            push(arguments, ["-blur".into(), format!("0x{sigma}")]);
        }
        Edit::Denoise { radius } => {
            if !matches!(radius, 1..=3) {
                return Err("降噪半径须在1–3之间".into());
            }
            let side = radius * 2 + 1;
            push(
                arguments,
                [
                    "-statistic".into(),
                    "Median".into(),
                    format!("{side}x{side}"),
                ],
            );
        }
        Edit::Sepia { percent } => {
            finite(*percent, 0.0, 100.0, "棕褐色强度")?;
            push(arguments, ["-sepia-tone".into(), format!("{percent}%")]);
        }
        Edit::Vignette { sigma } => {
            finite(*sigma, 1.0, 3000.0, "暗角范围")?;
            push(
                arguments,
                [
                    "-background".into(),
                    "black".into(),
                    "-vignette".into(),
                    format!("0x{sigma}"),
                ],
            );
        }
        Edit::Negate => push(arguments, ["-negate"]),
        Edit::Solarize { threshold } => {
            finite(*threshold, 0.0, 100.0, "曝光反转阈值")?;
            push(arguments, ["-solarize".into(), format!("{threshold}%")]);
        }
        Edit::Posterize { levels } => {
            if !(2..=256).contains(levels) {
                return Err("色阶数须在2–256之间".into());
            }
            push(arguments, ["-posterize".into(), levels.to_string()]);
        }
        Edit::Emboss { sigma } => {
            finite(*sigma, 0.1, 20.0, "浮雕半径")?;
            push(arguments, ["-emboss".into(), format!("0x{sigma}")]);
        }
        Edit::Edge { radius } => {
            finite(*radius, 0.1, 20.0, "边缘半径")?;
            push(arguments, ["-edge".into(), radius.to_string()]);
        }
        Edit::OilPaint { radius } => {
            finite(*radius, 0.1, 10.0, "油画半径")?;
            push(arguments, ["-paint".into(), radius.to_string()]);
        }
        Edit::Spread { radius } => {
            finite(*radius, 0.1, 50.0, "扩散半径")?;
            push(
                arguments,
                [
                    "-seed".into(),
                    "0".into(),
                    "-spread".into(),
                    radius.to_string(),
                ],
            );
        }
        Edit::Swirl { degrees } => {
            finite(*degrees, -360.0, 360.0, "旋涡角度")?;
            push(arguments, ["-swirl".into(), degrees.to_string()]);
        }
        Edit::Wave {
            amplitude,
            wavelength,
        } => {
            finite(*amplitude, 0.0, 1000.0, "波浪幅度")?;
            finite(*wavelength, 1.0, 10000.0, "波长")?;
            push(
                arguments,
                ["-wave".into(), format!("{amplitude}x{wavelength}")],
            );
        }
        Edit::Rotate {
            degrees,
            background,
        } => {
            finite(*degrees, -360.0, 360.0, "旋转角度")?;
            push(
                arguments,
                [
                    "-background".into(),
                    color(*background),
                    "-rotate".into(),
                    degrees.to_string(),
                ],
            );
        }
        Edit::LensDistortion { coefficients } => {
            for value in coefficients {
                finite(*value, -5.0, 5.0, "镜头校正参数")?;
            }
            push(
                arguments,
                [
                    "-virtual-pixel".into(),
                    "edge".into(),
                    "-distort".into(),
                    "Barrel".into(),
                    coefficients
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                ],
            );
        }
        Edit::Text {
            text,
            x,
            y,
            size,
            color: rgba,
        } => {
            if !(1..=2048).contains(size) {
                return Err("文字大小须在1–2048像素之间".into());
            }
            let font = ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/arial.ttf"]
                .into_iter()
                .map(Path::new)
                .find(|p| p.is_file())
                .ok_or("系统中没有可用于文字水印的字体")?;
            push(
                arguments,
                [
                    OsString::from("-font"),
                    font.as_os_str().to_owned(),
                    "-pointsize".into(),
                    size.to_string().into(),
                    "-fill".into(),
                    color(*rgba).into(),
                    "-gravity".into(),
                    "NorthWest".into(),
                    "-annotate".into(),
                    format!("{x:+}{y:+}").into(),
                    literal_text(text)?.into(),
                ],
            );
        }
        Edit::Watermark {
            path,
            x,
            y,
            opacity,
            width,
            height,
        } => {
            finite(*opacity, 0.0, 1.0, "水印不透明度")?;
            if !path.is_file()
                || path.metadata().map_err(|e| e.to_string())?.len() > MAX_OUTPUT_BYTES
            {
                return Err("水印文件不存在或超过512 MiB".into());
            }
            let mut reader = image::ImageReader::open(path)
                .map_err(|e| e.to_string())?
                .with_guessed_format()
                .map_err(|e| e.to_string())?;
            if reader.format().is_none() {
                reader.set_format(image::ImageFormat::from_path(path).map_err(|e| e.to_string())?);
            }
            let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
            if w == 0 || h == 0 || (w as u64) * (h as u64) * 8 > 64 * 1024 * 1024 {
                return Err("水印画布超过64 MiB高位预算".into());
            }
            let source = work.join(format!("watermark-source-{index}.img"));
            fs::copy(path, &source).map_err(|e| e.to_string())?;
            let name = format!("watermark-{index}.png");
            let target = work.join(&name);
            let mut prepare = vec![
                source.as_os_str().to_owned(),
                "-profile".into(),
                work.join("working.icc").into_os_string(),
                "-alpha".into(),
                "set".into(),
                "-channel".into(),
                "A".into(),
                "-evaluate".into(),
                "Multiply".into(),
                opacity.to_string().into(),
                "+channel".into(),
            ];
            if width.is_some() || height.is_some() {
                let out_w = width.unwrap_or_else(|| {
                    ((w as u64 * height.unwrap_or(h) as u64) / h as u64)
                        .max(1)
                        .min(u32::MAX as u64) as u32
                });
                let out_h = height.unwrap_or_else(|| {
                    ((h as u64 * width.unwrap_or(w) as u64) / w as u64)
                        .max(1)
                        .min(u32::MAX as u64) as u32
                });
                if out_w == 0
                    || out_h == 0
                    || (out_w as u64) * (out_h as u64) * 8 > 64 * 1024 * 1024
                {
                    return Err("水印输出画布超限".into());
                }
                push(
                    &mut prepare,
                    ["-resize".into(), format!("{out_w}x{out_h}!")],
                );
            }
            prepare.push(format!("PNG:{}", target.display()).into());
            run(&prepare, work)?;
            push(
                arguments,
                ["-draw".into(), format!("image over {x},{y} 0,0 '{name}'")],
            );
        }
    }
    Ok(())
}

fn safe_source(input: &Path, work: &Path) -> Result<PathBuf, String> {
    let size = crate::imaging::checked_input_size(input)?;
    if size > MAX_OUTPUT_BYTES {
        // A layered PSD can be gigabytes while its saved composite is small.
        // Read the validated PSD directly: copying it would defeat seek-based
        // layer skipping. An absolute path cannot become an option or coder;
        // the existing filename:literal setting preserves [] and % in names.
        // Every PSD probe/decode/render still selects only composite scene [0].
        return input.canonicalize().map_err(|e| e.to_string());
    }
    let extension = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("img")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect::<String>();
    let destination = work.join(format!("source.{extension}"));
    fs::copy(input, &destination).map_err(|e| e.to_string())?;
    Ok(destination)
}

/// Save a high-depth image/animation. Caller runs this in the existing memory-limited worker.
pub fn render(
    input: &Path,
    page: Option<u32>,
    edits: &[Edit],
    settings: &ExportSettings,
    raw: &RawSettings,
    work_dir: &Path,
    destination: &Path,
) -> Result<RenderSummary, String> {
    if edits.len() > MAX_EDITS {
        return Err("编辑步骤超过128步".into());
    }
    if !(1..=100).contains(&settings.quality) || !matches!(settings.depth, 8 | 16) {
        return Err("导出质量或位深无效".into());
    }
    let photoshop = crate::imaging::is_photoshop(&input.to_string_lossy());
    let all = settings.animation == AnimationMode::AllFrames && !photoshop;
    if all && !matches!(settings.format, OutputFormat::Gif | OutputFormat::WebP) {
        return Err("完整动画输出请选择GIF或WebP".into());
    }
    if crate::imaging::is_raw(&destination.to_string_lossy()) {
        return Err("不覆盖RAW源文件，请另存图像或编辑记录".into());
    }
    if crate::imaging::is_photoshop(&destination.to_string_lossy()) {
        return Err("PSD/PSB只读，请另存PNG/TIFF等图像或编辑记录".into());
    }
    raw.validate()?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err("输出文件夹不存在".into());
    }
    let work = tempfile::Builder::new()
        .prefix("precision-")
        .tempdir_in(work_dir)
        .map_err(|e| e.to_string())?;
    let work = work.path();
    let is_raw = crate::imaging::is_raw(&input.to_string_lossy());
    let source = if is_raw {
        let path = work.join("raw16.png");
        crate::imaging::raw16_to_png(input, &path, raw, MAX_OUTPUT_BYTES as usize)?;
        path
    } else {
        safe_source(input, work)?
    };
    let metadata_source = if photoshop {
        PathBuf::from(format!("{}[0]", source.display()))
    } else {
        source.clone()
    };
    let (source_width, source_height, _, _) = metadata(&metadata_source, work)?;
    if (source_width as u64) * (source_height as u64) * 8 > MAX_OUTPUT_BYTES {
        return Err("源画布超过512 MiB高位RGBA预算".into());
    }
    let working = if is_raw {
        raw_profile(raw, false)?
    } else {
        encode_profile(&moxcms::ColorProfile::new_srgb())?
    };
    let linear = if is_raw {
        raw_profile(raw, true)?
    } else {
        let mut p = moxcms::ColorProfile::new_srgb();
        let trc = moxcms::curve_from_gamma(1.0);
        p.red_trc = Some(trc.clone());
        p.green_trc = Some(trc.clone());
        p.blue_trc = Some(trc);
        p.cicp = None;
        encode_profile(&p)?
    };
    fs::write(work.join("working.icc"), working).map_err(|e| e.to_string())?;
    fs::write(work.join("linear.icc"), linear).map_err(|e| e.to_string())?;
    let mut arguments = Vec::new();
    let mut source_arg = source.as_os_str().to_owned();
    if !all {
        source_arg.push(format!(
            "[{}]",
            if photoshop { 0 } else { page.unwrap_or(0) }
        ));
    }
    arguments.push(source_arg);
    push(&mut arguments, ["-auto-orient"]);
    if all {
        push(&mut arguments, ["-coalesce"]);
    }
    if !is_raw {
        push(
            &mut arguments,
            ["-profile".into(), work.join("working.icc").into_os_string()],
        );
    }
    if is_raw && raw.exposure_ev != 0.0 {
        add_edit(
            &mut arguments,
            &Edit::Exposure {
                ev: raw.exposure_ev,
            },
            work,
            0,
        )?;
    }
    for (index, edit) in edits.iter().enumerate() {
        add_edit(&mut arguments, edit, work, index)?;
    }
    if !settings.preserve_metadata {
        push(&mut arguments, ["+profile", "*", "+set", "*"]);
        // Color interpretation is essential even when descriptive metadata is removed.
        push(
            &mut arguments,
            ["-profile".into(), work.join("working.icc").into_os_string()],
        );
    }
    let low_depth = edits.iter().rev().find_map(|e| {
        if let Edit::ColorDepth { mode } = e {
            Some(*mode)
        } else {
            None
        }
    });
    if matches!(
        low_depth,
        Some(
            ColorDepth::BlackWhite1
                | ColorDepth::Gray4
                | ColorDepth::Gray8
                | ColorDepth::Indexed4
                | ColorDepth::Indexed8
        )
    ) && settings.format != OutputFormat::Png
    {
        return Err("精确低位/索引色输出请选择PNG".into());
    }
    if low_depth == Some(ColorDepth::Rgb15) && settings.format != OutputFormat::Bmp {
        return Err("精确15位RGB555输出请选择BMP".into());
    }
    if matches!(low_depth, Some(ColorDepth::Gray16 | ColorDepth::Rgb48))
        && !matches!(settings.format, OutputFormat::Png | OutputFormat::Tiff)
    {
        return Err("精确16/48位输出请选择PNG或TIFF".into());
    }
    if low_depth.is_none() {
        push(
            &mut arguments,
            ["-depth".into(), settings.depth.to_string()],
        );
    }
    push(
        &mut arguments,
        ["-quality".into(), settings.quality.to_string()],
    );
    if settings.format == OutputFormat::Png {
        push(&mut arguments, ["-define", "png:include-chunks=iCCP"]);
        let definition = match low_depth {
            Some(ColorDepth::Rgba8) => Some((8, 6)),
            Some(ColorDepth::BlackWhite1) => Some((1, 0)),
            Some(ColorDepth::Gray4) => Some((4, 0)),
            Some(ColorDepth::Gray8) => Some((8, 0)),
            Some(ColorDepth::Indexed4) => Some((4, 3)),
            Some(ColorDepth::Indexed8) => Some((8, 3)),
            Some(ColorDepth::Rgb24) => Some((8, 2)),
            Some(ColorDepth::Gray16) => Some((16, 0)),
            Some(ColorDepth::Rgb48) => Some((16, 2)),
            None => {
                push(
                    &mut arguments,
                    [
                        "-define".into(),
                        format!("png:bit-depth={}", settings.depth),
                        "-set".into(),
                        "option:png:color-type".into(),
                        "%[fx:minima.a<1?6:2]".into(),
                    ],
                );
                None
            }
            Some(ColorDepth::Rgb15) => None,
        };
        if let Some((depth, color)) = definition {
            push(
                &mut arguments,
                [
                    "-define".into(),
                    format!("png:bit-depth={depth}"),
                    "-define".into(),
                    format!("png:color-type={color}"),
                ],
            );
        }
    }
    if low_depth == Some(ColorDepth::Rgb15) {
        push(
            &mut arguments,
            ["-define", "bmp:subtype=RGB555", "-type", "TrueColor"],
        );
    }
    if settings.format == OutputFormat::WebP {
        push(&mut arguments, ["-define", "webp:lossless=true"]);
    }
    if all {
        let delays = animation_delays(&source, work)?;
        let mut expression = String::from("%[fx:");
        for (index, delay) in delays
            .iter()
            .enumerate()
            .take(delays.len().saturating_sub(1))
        {
            expression.push_str(&format!("t=={index}?{delay}:"));
        }
        expression.push_str(&format!("{}]x1000", delays.last().copied().unwrap_or(10)));
        push(
            &mut arguments,
            [
                "-set".into(),
                "delay".into(),
                expression,
                "-layers".into(),
                "Optimize".into(),
            ],
        );
    }
    let stage = tempfile::Builder::new()
        .prefix(".rustview-render-")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    arguments.push(format!("{}:{}", settings.format.coder(), stage.path().display()).into());
    run(&arguments, work)?;
    if stage.path().metadata().map_err(|e| e.to_string())?.len() > MAX_OUTPUT_BYTES {
        return Err("编码输出超过512 MiB，原文件未改变".into());
    }
    let (width, height, depth, output_frames) = metadata(stage.path(), work)?;
    if (width as u64) * (height as u64) * 8 > MAX_OUTPUT_BYTES {
        return Err("输出画布超过512 MiB高位RGBA预算，原文件未改变".into());
    }
    let frames = if all { output_frames } else { 1 };
    stage.as_file().sync_all().map_err(|e| e.to_string())?;
    editing::publish(stage, destination)?;
    Ok(RenderSummary {
        width,
        height,
        frames,
        depth,
        note: format!("ImageMagick Q16-HDRI · {frames}帧 · 高位源直接处理，不经过RGBA8预览回写"),
    })
}

fn animation_delays(path: &Path, work: &Path) -> Result<Vec<u32>, String> {
    // ImageMagick's %T rounds WebP's milliseconds to centiseconds. The existing
    // streaming decoder preserves exact milliseconds with one bounded frame.
    if let Some(mut stream) = crate::animation::open(path, 128 * 1024 * 1024)? {
        stream.set_color_management(false);
        let mut delays = Vec::new();
        while let Some(frame) = stream.next_frame()? {
            if delays.len() == 4096 {
                return Err("动画超过4096帧导出限额".into());
            }
            delays.push(frame.duration_ms);
        }
        if !delays.is_empty() {
            return Ok(delays);
        }
    }
    let stdout = work.join("delays.txt");
    run_command(
        &[
            "-ping".into(),
            "-format".into(),
            "%T\n".into(),
            path.as_os_str().to_owned(),
        ],
        work,
        Some("identify"),
        Some(&stdout),
    )?;
    if stdout.metadata().map_err(|e| e.to_string())?.len() > 128 * 1024 {
        return Err("动画时序索引超过预算".into());
    }
    let text = fs::read_to_string(stdout).map_err(|e| e.to_string())?;
    let mut delays = Vec::new();
    for line in text.lines() {
        if delays.len() == 4096 {
            return Err("动画超过4096帧导出限额".into());
        }
        let ticks = line.trim().parse::<f64>().map_err(|_| "动画帧延时无效")?;
        if !ticks.is_finite() || ticks < 0.0 || ticks > u32::MAX as f64 / 10.0 {
            return Err("动画帧延时超出范围".into());
        }
        delays.push((ticks * 10.0).round().max(10.0) as u32);
    }
    if delays.is_empty() {
        return Err("动画没有可导出帧".into());
    }
    Ok(delays)
}

fn metadata(path: &Path, work: &Path) -> Result<(u32, u32, u8, u32), String> {
    let stdout = work.join("identify.txt");
    run_command(
        &[
            "-ping".into(),
            "-format".into(),
            "%w %h %z %n\n".into(),
            path.as_os_str().to_owned(),
        ],
        work,
        Some("identify"),
        Some(&stdout),
    )?;
    if stdout.metadata().map_err(|e| e.to_string())?.len() > 128 * 1024 {
        return Err("图像元数据超过预算".into());
    }
    let content = fs::read_to_string(stdout).map_err(|e| e.to_string())?;
    let values = content
        .lines()
        .next()
        .ok_or("没有图像元数据")?
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "图像元数据无效")?;
    if values.len() != 4 || values[0] == 0 || values[1] == 0 || values[3] == 0 || values[3] > 4096 {
        return Err("图像元数据或帧数无效".into());
    }
    Ok((values[0], values[1], values[2].min(255) as u8, values[3]))
}

pub fn decode_fallback(
    input: &Path,
    max_pixels: u64,
    max_bytes: usize,
    managed: bool,
) -> Result<crate::imaging::DecodedImage, String> {
    let work = tempfile::Builder::new()
        .prefix("rustview-codec-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let work = work.path();
    let source = safe_source(input, work)?;
    // PSD/PSB scene zero is the composite. Restrict the probe as well as the
    // decode so ImageMagick can skip layer pixels instead of listing them.
    let first = PathBuf::from(format!("{}[0]", source.display()));
    let (w, h, _, _) = metadata(&first, work)?;
    let allowed = (w as u64)
        .checked_mul(h as u64)
        .filter(|n| *n <= max_pixels)
        .and_then(|n| n.checked_mul(4))
        .is_some_and(|n| n <= max_bytes as u64);
    if !allowed {
        return Err("备用解码图像超过像素或RGBA内存预算".into());
    }
    let output = work.join("decoded.png");
    let mut args = vec![first.into_os_string(), "-auto-orient".into()];
    if managed {
        push(
            &mut args,
            [
                "-profile".into(),
                engine_dir()?.join("sRGB.icc").into_os_string(),
            ],
        );
    }
    push(
        &mut args,
        [
            OsString::from("-depth"),
            "8".into(),
            format!("PNG:{}", output.display()).into(),
        ],
    );
    run(&args, work)?;
    if output.metadata().map_err(|e| e.to_string())?.len() > MAX_OUTPUT_BYTES {
        return Err("新格式中间输出超过512 MiB".into());
    }
    let mut reader = image::ImageReader::open(&output)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(max_bytes as u64);
    limits.max_image_width = Some(max_pixels.min(u32::MAX as u64) as u32);
    limits.max_image_height = limits.max_image_width;
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?.into_rgba8();
    if image.len() > max_bytes {
        return Err("新格式解码结果超过RGBA预算".into());
    }
    Ok(crate::imaging::DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        note: format!(
            "ImageMagick Q16-HDRI · {}解码{}{}",
            input.extension().and_then(|e| e.to_str()).unwrap_or("图像"),
            if crate::imaging::is_photoshop(&input.to_string_lossy()) {
                " · 文档合成预览"
            } else {
                ""
            },
            if managed { " · ICC→sRGB" } else { "" }
        ),
    })
}

pub fn apply_native(image: EditedImage, edit: &Edit) -> Result<EditedImage, String> {
    let work = tempfile::Builder::new()
        .prefix("rustview-filter-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let source = work.path().join("preview.png");
    image::save_buffer_with_format(
        &source,
        &image.rgba,
        image.width,
        image.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| e.to_string())?;
    let output = work.path().join("filtered.png");
    render(
        &source,
        None,
        std::slice::from_ref(edit),
        &ExportSettings {
            depth: 8,
            ..Default::default()
        },
        &RawSettings::default(),
        work.path(),
        &output,
    )?;
    let decoded = crate::imaging::decode(&output, 200_000_000, editing::MAX_BYTES, false)?;
    Ok(EditedImage {
        width: decoded.width,
        height: decoded.height,
        rgba: decoded.rgba,
        color_depth: image.color_depth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png16(path: &Path) {
        let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), 2, 1);
        encoder.set_depth(png::BitDepth::Sixteen);
        encoder.set_color(png::ColorType::Rgb);
        let mut w = encoder.write_header().unwrap();
        w.write_image_data(&[0, 1, 1, 2, 4, 3, 128, 7, 254, 1, 255, 255])
            .unwrap();
        w.finish().unwrap();
    }
    #[test]
    fn high_depth_export_keeps_low_bits_and_source_and_failure_target() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("@input[0]中文.png");
        let output = d.path().join("out.png");
        png16(&source);
        let before = fs::read(&source).unwrap();
        let summary = render(
            &source,
            None,
            &[],
            &ExportSettings::default(),
            &RawSettings::default(),
            d.path(),
            &output,
        )
        .unwrap();
        assert_eq!((summary.width, summary.height, summary.frames), (2, 1, 1));
        let mut decoder =
            png::Decoder::new(std::io::BufReader::new(fs::File::open(&output).unwrap()))
                .read_info()
                .unwrap();
        assert_eq!(decoder.info().bit_depth, png::BitDepth::Sixteen);
        let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
        let frame = decoder.next_frame(&mut decoded).unwrap();
        decoded.truncate(frame.buffer_size());
        assert!(
            decoded.chunks_exact(2).any(|p| p[0] != p[1]),
            "real16bit lower samples must not be expanded RGBA8"
        );
        assert_eq!(before, fs::read(&source).unwrap());
        let saved = fs::read(&output).unwrap();
        assert!(render(
            &source,
            None,
            &[Edit::Blur { sigma: f32::NAN }],
            &ExportSettings::default(),
            &RawSettings::default(),
            d.path(),
            &output
        )
        .is_err());
        assert_eq!(saved, fs::read(output).unwrap());
    }
    #[test]
    fn eight_bit_source_exports_true_sixteen_bit_png_with_icc() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("eight.png");
        let output = d.path().join("sixteen.png");
        image::RgbaImage::from_pixel(6, 4, image::Rgba([90, 120, 180, 128]))
            .save(&source)
            .unwrap();
        let summary = render(
            &source,
            None,
            &[],
            &ExportSettings {
                depth: 16,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &output,
        )
        .unwrap();
        assert_eq!(summary.depth, 16);
        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(output).unwrap()))
            .read_info()
            .unwrap();
        assert_eq!(decoder.info().bit_depth, png::BitDepth::Sixteen);
        assert_eq!(decoder.info().color_type, png::ColorType::Rgba);
        assert!(decoder.info().icc_profile.is_some());
    }
    #[test]
    #[ignore = "Public HEIC fixture is downloaded separately and excluded from packages"]
    fn public_heic_exports_true_sixteen_bit_png_with_icc() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-output/modern-raw-samples/libheif-example.heic");
        let d = tempfile::tempdir().unwrap();
        let output = d.path().join("heic16.png");
        let before = fs::read(&source).unwrap();
        let summary = render(
            &source,
            None,
            &[],
            &ExportSettings {
                depth: 16,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &output,
        )
        .unwrap();
        assert_eq!(
            (summary.width, summary.height, summary.depth),
            (1280, 854, 16)
        );
        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(output).unwrap()))
            .read_info()
            .unwrap();
        assert_eq!(decoder.info().bit_depth, png::BitDepth::Sixteen);
        assert!(decoder.info().icc_profile.is_some());
        assert_eq!(before, fs::read(source).unwrap());
    }
    #[test]
    fn advanced_filters_and_literal_text_preserve_alpha_and_do_not_read_at_file() {
        let image = EditedImage {
            width: 40,
            height: 20,
            rgba: vec![128; 40 * 20 * 4],
            color_depth: ColorDepth::Rgba8,
        };
        let adjusted = apply_native(
            image,
            &Edit::WhiteBalance {
                red: 1.2,
                green: 1.0,
                blue: 0.8,
            },
        )
        .unwrap();
        assert_eq!((adjusted.width, adjusted.height), (40, 20));
        assert!(adjusted.rgba.chunks_exact(4).all(|p| p[3] == 128));
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("s.png");
        image::RgbaImage::from_pixel(220, 80, image::Rgba([255, 255, 255, 255]))
            .save(&source)
            .unwrap();
        let out = d.path().join("text.png");
        let text = Edit::Text {
            text: "@does-not-exist %[fx:1+1]".into(),
            x: 0,
            y: 20,
            size: 16,
            color: [0, 0, 0, 255],
        };
        render(
            &source,
            None,
            &[text],
            &ExportSettings {
                depth: 8,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &out,
        )
        .unwrap();
        assert!(out.is_file());
    }
    #[test]
    fn watermark_and_long_script_keep_literal_arguments() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("source.png");
        let mark = d.path().join("@mark[0].png");
        let out = d.path().join("marked.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]))
            .save(&source)
            .unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&mark)
            .unwrap();
        render(
            &source,
            None,
            &[Edit::Watermark {
                path: mark,
                x: 1,
                y: 2,
                opacity: 0.5,
                width: Some(4),
                height: Some(3),
            }],
            &ExportSettings {
                depth: 8,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &out,
        )
        .unwrap();
        let pixels = image::open(&out).unwrap().into_rgba8();
        assert!(pixels.get_pixel(2, 3)[0] > 100);
        assert_eq!(pixels.get_pixel(0, 0), &image::Rgba([0, 0, 0, 255]));
        let out2 = d.path().join("width-only.png");
        render(
            &source,
            None,
            &[Edit::Watermark {
                path: d.path().join("@mark[0].png"),
                x: 1,
                y: 2,
                opacity: 0.5,
                width: Some(4),
                height: None,
            }],
            &ExportSettings {
                depth: 8,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &out2,
        )
        .unwrap();
        let pixels = image::open(out2).unwrap().into_rgba8();
        assert!(pixels.get_pixel(4, 5)[0] > 100);
        assert_eq!(pixels.get_pixel(7, 7)[0], 0);
        let scripted = d.path().join("script.png");
        let mut arguments = vec![
            source.as_os_str().to_owned(),
            "-set".into(),
            "comment".into(),
        ];
        arguments.push(format!("literal \\\" %% @ \\n {}", "x".repeat(25000)).into());
        arguments.push(format!("PNG:{}", scripted.display()).into());
        run(&arguments, d.path()).unwrap();
        assert_eq!(image::open(scripted).unwrap().width(), 8);
    }
    #[test]
    fn full_animation_edit_keeps_each_frame_delays_and_loops() {
        let d = tempfile::tempdir().unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/animation-previous.gif");
        let output = d.path().join("edited.webp");
        let summary = render(
            &source,
            None,
            &[Edit::Resize {
                width: 8,
                height: 4,
            }],
            &ExportSettings {
                format: OutputFormat::WebP,
                animation: AnimationMode::AllFrames,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &output,
        )
        .unwrap();
        assert_eq!((summary.width, summary.height, summary.frames), (8, 4, 3));
        let mut stream = crate::animation::open(&output, 1024).unwrap().unwrap();
        assert_eq!(stream.repeat(), crate::animation::RepeatCount::Finite(2));
        let mut delays = Vec::new();
        while let Some(f) = stream.next_frame().unwrap() {
            delays.push(f.duration_ms);
        }
        assert_eq!(delays, [30, 50, 10]);
    }
    #[test]
    fn webp_millisecond_timing_and_composited_pixels_survive_export() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("odd.webp");
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/animation-blend.webp");
        let mut bytes = fs::read(fixture).unwrap();
        let (mut offset, mut index) = (12usize, 0usize);
        while offset + 8 <= bytes.len() {
            let length =
                u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            if &bytes[offset..offset + 4] == b"ANMF" {
                bytes[offset + 20..offset + 23]
                    .copy_from_slice(&[13u32, 27, 10][index].to_le_bytes()[..3]);
                index += 1;
            }
            offset += 8 + length + (length & 1);
        }
        fs::write(&source, bytes).unwrap();
        let output = d.path().join("out.webp");
        render(
            &source,
            None,
            &[],
            &ExportSettings {
                format: OutputFormat::WebP,
                animation: AnimationMode::AllFrames,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &output,
        )
        .unwrap();
        let mut before = crate::animation::open(&source, 1024).unwrap().unwrap();
        let mut after = crate::animation::open(&output, 1024).unwrap().unwrap();
        for delay in [13, 27, 10] {
            let a = before.next_frame().unwrap().unwrap();
            let b = after.next_frame().unwrap().unwrap();
            assert_eq!(b.duration_ms, delay);
            for (x, y) in a.rgba.chunks_exact(4).zip(b.rgba.chunks_exact(4)) {
                assert_eq!(x[3], y[3]);
                if x[3] != 0 {
                    assert_eq!(x, y);
                }
            }
        }
        assert!(after.next_frame().unwrap().is_none());
    }
    #[test]
    fn genuine_raw16_is_encoded_and_budget_is_checked() {
        let d = tempfile::tempdir().unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("native/LibRaw/testdata/synthetic.dng");
        let out = d.path().join("raw16.png");
        let summary =
            crate::imaging::raw16_to_png(&source, &out, &RawSettings::default(), 1024 * 1024)
                .unwrap();
        assert_eq!((summary.width, summary.height), (32, 24));
        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(&out).unwrap()))
            .read_info()
            .unwrap();
        assert_eq!(decoder.info().bit_depth, png::BitDepth::Sixteen);
        assert!(decoder.info().icc_profile.is_some());
        assert!(crate::imaging::raw16_to_png(
            &source,
            &d.path().join("limited.png"),
            &RawSettings::default(),
            10
        )
        .is_err());
    }
    #[test]
    fn wide_gamut_raw_output_keeps_matching_icc_when_metadata_is_removed() {
        let d = tempfile::tempdir().unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("native/LibRaw/testdata/synthetic.dng");
        for color in [2, 4] {
            let raw = RawSettings {
                output_color: color,
                ..Default::default()
            };
            let output = d.path().join(format!("{color}.png"));
            render(
                &source,
                None,
                &[],
                &ExportSettings {
                    preserve_metadata: false,
                    ..Default::default()
                },
                &raw,
                d.path(),
                &output,
            )
            .unwrap();
            let reader =
                png::Decoder::new(std::io::BufReader::new(fs::File::open(output).unwrap()))
                    .read_info()
                    .unwrap();
            let profile =
                moxcms::ColorProfile::new_from_slice(reader.info().icc_profile.as_ref().unwrap())
                    .unwrap();
            let expected =
                moxcms::ColorProfile::new_from_slice(&raw_profile(&raw, false).unwrap()).unwrap();
            assert_eq!(profile.red_colorant, expected.red_colorant);
            assert_eq!(profile.green_colorant, expected.green_colorant);
            assert_eq!(profile.blue_colorant, expected.blue_colorant);
        }
    }
    #[test]
    fn icc_tags_are_aligned_d50_and_native_reader_accepts_without_warnings() {
        let d = tempfile::tempdir().unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("native/LibRaw/testdata/synthetic.dng");
        let expected = [0x0000f6d6_u32, 0x00010000, 0x0000d32d]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        for color in [1, 2, 4] {
            let raw = RawSettings {
                output_color: color,
                ..Default::default()
            };
            for linear in [false, true] {
                let icc = raw_profile(&raw, linear).unwrap();
                assert_eq!(&icc[68..80], &expected);
                let count = u32::from_be_bytes(icc[128..132].try_into().unwrap()) as usize;
                for i in 0..count {
                    let e = 132 + i * 12;
                    let offset = u32::from_be_bytes(icc[e + 4..e + 8].try_into().unwrap()) as usize;
                    assert_eq!(offset % 4, 0);
                    if &icc[e..e + 4] == b"wtpt" {
                        assert_eq!(&icc[offset + 8..offset + 20], &expected);
                    }
                }
            }
            let output = d.path().join(format!("{color}.png"));
            crate::imaging::raw16_to_png(&source, &output, &raw, 1024 * 1024).unwrap();
            run_command(
                &[
                    "-regard-warnings".into(),
                    "-ping".into(),
                    output.into_os_string(),
                ],
                d.path(),
                Some("identify"),
                None,
            )
            .unwrap();
        }
    }
    #[test]
    fn png_chooses_channels_from_final_alpha_after_raw_edits() {
        let d = tempfile::tempdir().unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("native/LibRaw/testdata/synthetic.dng");
        for (index, edits) in [
            vec![],
            vec![Edit::Rotate {
                degrees: 30.0,
                background: [0, 0, 0, 0],
            }],
            vec![Edit::Wave {
                amplitude: 4.0,
                wavelength: 16.0,
            }],
            vec![Edit::LensDistortion {
                coefficients: [0.0, 0.0, 0.1, 0.9],
            }],
        ]
        .into_iter()
        .enumerate()
        {
            let output = d.path().join(format!("{index}.png"));
            render(
                &source,
                None,
                &edits,
                &ExportSettings::default(),
                &RawSettings::default(),
                d.path(),
                &output,
            )
            .unwrap();
            let rgba = image::open(&output).unwrap().into_rgba8();
            let transparent = rgba.pixels().any(|p| p[3] < 255);
            let bytes = fs::read(output).unwrap();
            assert_eq!(bytes[24], 16);
            assert_eq!(bytes[25], if transparent { 6 } else { 2 });
        }
    }
    #[test]
    fn modern_codec_fallback_is_bounded_and_handles_avif_jxl() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("rgb.png");
        image::RgbaImage::from_pixel(6, 4, image::Rgba([90, 120, 180, 255]))
            .save(&source)
            .unwrap();
        for extension in ["avif", "jxl"] {
            let encoded = d.path().join(format!("file.{extension}"));
            run(
                &[
                    source.as_os_str().to_owned(),
                    format!("{}:{}", extension.to_ascii_uppercase(), encoded.display()).into(),
                ],
                d.path(),
            )
            .unwrap();
            let image = crate::imaging::decode(&encoded, 100, 1000, false).unwrap();
            assert_eq!((image.width, image.height), (6, 4));
            assert!(crate::imaging::decode(&encoded, 10, 1000, false).is_err());
        }
    }
    #[test]
    fn photoshop_composites_preserve_pixels_alpha_and_source() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd");
        let cases: &[(&str, &[u8])] = &[
            ("rgb8.psd", &[255, 0, 0, 255, 0, 255, 0, 255]),
            ("rgb-rle.psd", &[255, 0, 0, 255, 0, 255, 0, 255]),
            ("rgba8.psd", &[255, 0, 0, 128, 0, 255, 0, 255]),
            ("rgb16.psd", &[64, 128, 191, 255, 255, 0, 0, 255]),
            ("cmyk8.psd", &[255, 255, 255, 255, 0, 255, 255, 255]),
            ("rgb-icc.psd", &[90, 120, 180, 255, 90, 120, 180, 255]),
            // The actual layer is blue; only the saved composite is orange.
            ("layered.psd", &[240, 120, 30, 255, 240, 120, 30, 255]),
            ("layered.psb", &[240, 120, 30, 255, 240, 120, 30, 255]),
        ];
        for (name, expected) in cases {
            let source = fixtures.join(name);
            let original = fs::read(&source).unwrap();
            // PNG max_alloc includes codec scratch/metadata, not just RGBA.
            // Exact pixel/RGBA rejection is covered separately below.
            let image = crate::imaging::decode(&source, 2, 1024, false).unwrap();
            assert_eq!((image.width, image.height), (2, 1), "{name}");
            assert_eq!(&image.rgba, expected, "{name}");
            assert!(image.note.contains("文档合成预览"), "{name}");
            assert_eq!(fs::read(source).unwrap(), original, "{name}");
        }
    }
    #[test]
    fn large_photoshop_source_seeks_to_composite_without_copying_or_changing_budgets() {
        use std::io::{Seek, SeekFrom, Write};
        let directory = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let fixture = include_bytes!("../tests/fixtures/psd/rgb8.psd");
        let layer_bytes = 512 * 1024 * 1024_u64;
        for (name, version) in [("中文[7]100% &.PSD", 1_u16), ("-literal[0].PSB", 2_u16)] {
            let source = directory.path().join(name);
            let mut file = fs::File::create(&source).unwrap();
            let mut header = fixture[..34].to_vec();
            header[4..6].copy_from_slice(&version.to_be_bytes());
            file.write_all(&header).unwrap();
            if version == 1 {
                file.write_all(&(layer_bytes as u32).to_be_bytes()).unwrap();
            } else {
                file.write_all(&layer_bytes.to_be_bytes()).unwrap();
            }
            let composite = file.stream_position().unwrap() + layer_bytes;
            file.seek(SeekFrom::Start(composite)).unwrap();
            file.write_all(&fixture[38..]).unwrap();
            drop(file);
            let size = fs::metadata(&source).unwrap().len();
            assert!(size > MAX_OUTPUT_BYTES);
            assert_eq!(crate::imaging::checked_input_size(&source).unwrap(), size);
            assert_eq!(
                safe_source(&source, work.path()).unwrap(),
                source.canonicalize().unwrap()
            );
            assert!(!work.path().join("source.PSD").exists());
            assert!(!work.path().join("source.PSB").exists());
            let image = crate::imaging::decode(&source, 2, 1024, false).unwrap();
            assert_eq!((image.width, image.height), (2, 1));
            assert_eq!(image.rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
            for (pixels, bytes) in [(1, 1024), (2, 7)] {
                let error = crate::imaging::decode(&source, pixels, bytes, false)
                    .err()
                    .unwrap();
                assert!(error.contains("超过像素或RGBA内存预算"), "{error}");
            }
            let output = work.path().join(format!("{name}.png"));
            let summary = render(
                &source,
                None,
                &[],
                &ExportSettings::default(),
                &RawSettings::default(),
                work.path(),
                &output,
            )
            .unwrap();
            assert_eq!((summary.width, summary.height, summary.frames), (2, 1, 1));
            assert_eq!(
                image::open(&output).unwrap().into_rgba8().as_raw(),
                &[255, 0, 0, 255, 0, 255, 0, 255]
            );
            assert_eq!(fs::metadata(&source).unwrap().len(), size);
            fs::rename(
                &source,
                directory.path().join(format!("moved-{version}.psd")),
            )
            .unwrap();
        }
    }
    #[test]
    fn photoshop_decode_rejects_pixel_and_rgba_budgets_and_writing() {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd/layered.psd");
        for (pixels, bytes) in [(1, 8), (2, 7)] {
            let error = crate::imaging::decode(&source, pixels, bytes, false)
                .err()
                .unwrap();
            assert!(error.contains("超过像素或RGBA内存预算"), "{error}");
        }
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("forbidden.psd");
        let error = run(
            &[
                format!("{}[0]", source.display()).into(),
                format!("PSD:{}", output.display()).into(),
            ],
            directory.path(),
        )
        .unwrap_err();
        assert!(error.contains("security policy"), "{error}");
        assert!(OutputFormat::from_path(&output).is_err());
    }
    #[test]
    fn photoshop_render_always_uses_composite_and_never_overwrites_psd() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd");
        let directory = tempfile::tempdir().unwrap();
        for name in ["layered.psd", "layered.psb"] {
            let source = directory.path().join(name);
            fs::copy(fixtures.join(name), &source).unwrap();
            let original = fs::read(&source).unwrap();
            for format in [OutputFormat::Png, OutputFormat::WebP] {
                let output = directory
                    .path()
                    .join(format!("{name}.{}", format.extension()));
                let settings = ExportSettings {
                    format,
                    depth: 8,
                    animation: AnimationMode::AllFrames,
                    ..Default::default()
                };
                let summary = render(
                    &source,
                    Some(1),
                    &[],
                    &settings,
                    &RawSettings::default(),
                    directory.path(),
                    &output,
                )
                .unwrap();
                assert_eq!(summary.frames, 1, "{name}");
                assert_eq!(
                    image::open(&output).unwrap().into_rgba8().as_raw(),
                    &[240, 120, 30, 255, 240, 120, 30, 255],
                    "{name}"
                );
                let (_, _, _, frames) = metadata(&output, directory.path()).unwrap();
                assert_eq!(frames, 1, "{name}");
            }
            let error = render(
                &source,
                None,
                &[],
                &ExportSettings::default(),
                &RawSettings::default(),
                directory.path(),
                &source,
            )
            .unwrap_err();
            assert!(error.contains("PSD/PSB只读"), "{error}");
            assert_eq!(fs::read(&source).unwrap(), original, "{name}");
        }
    }
    #[test]
    fn explicit_color_depth_uses_real_container_bits_and_rejects_conflicts() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("source.png");
        png16(&source);
        for (mode, depth, color) in [
            (ColorDepth::Rgba8, 8, 6),
            (ColorDepth::BlackWhite1, 1, 0),
            (ColorDepth::Gray4, 4, 0),
            (ColorDepth::Gray8, 8, 0),
            (ColorDepth::Indexed4, 4, 3),
            (ColorDepth::Indexed8, 8, 3),
            (ColorDepth::Rgb24, 8, 2),
            (ColorDepth::Gray16, 16, 0),
            (ColorDepth::Rgb48, 16, 2),
        ] {
            let output = d.path().join(format!("{mode:?}.png"));
            render(
                &source,
                None,
                &[Edit::ColorDepth { mode }],
                &ExportSettings {
                    depth: 8,
                    ..Default::default()
                },
                &RawSettings::default(),
                d.path(),
                &output,
            )
            .unwrap();
            let data = fs::read(output).unwrap();
            assert_eq!((data[24], data[25]), (depth, color), "{mode:?}");
        }
        let output = d.path().join("rgb555.bmp");
        render(
            &source,
            None,
            &[Edit::ColorDepth {
                mode: ColorDepth::Rgb15,
            }],
            &ExportSettings {
                format: OutputFormat::Bmp,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &output,
        )
        .unwrap();
        let data = fs::read(output).unwrap();
        assert_eq!(u16::from_le_bytes([data[28], data[29]]), 16);
        assert!(render(
            &source,
            None,
            &[Edit::ColorDepth {
                mode: ColorDepth::Gray4
            }],
            &ExportSettings {
                format: OutputFormat::Jpeg,
                ..Default::default()
            },
            &RawSettings::default(),
            d.path(),
            &d.path().join("bad.jpg")
        )
        .is_err());
    }
    #[test]
    #[ignore = "Public real-camera samples are downloaded separately and excluded from packages"]
    fn real_brand_samples_raw16_and_heic() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-output/modern-raw-samples");
        let out = tempfile::tempdir().unwrap();
        for name in [
            "Sony-a7III.ARW",
            "Canon-EOS-R.CR3",
            "Nikon-Z6.NEF",
            "Apple-iPhone-12-Pro.DNG",
        ] {
            let input = root.join(name);
            let preview =
                crate::imaging::decode(&input, 200_000_000, 512 * 1024 * 1024, false).unwrap();
            let summary = crate::imaging::raw16_to_png(
                &input,
                &out.path().join(format!("{name}.png")),
                &RawSettings::default(),
                512 * 1024 * 1024,
            )
            .unwrap();
            assert!(summary.width > 1000 && summary.height > 1000);
            println!(
                "{name}: preview={}x{}, full16={}x{}",
                preview.width, preview.height, summary.width, summary.height
            );
        }
        let heic = crate::imaging::decode(
            &root.join("libheif-example.heic"),
            200_000_000,
            512 * 1024 * 1024,
            false,
        )
        .unwrap();
        println!("HEIC={}x{}", heic.width, heic.height);
        assert!(heic.width > 100 && heic.height > 100);
    }
}
