//! Run bundled Tesseract for manual recognition and automatic text selection.
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_SIDE: u32 = 4096;
const MAX_PIXELS: usize = 12 * 1024 * 1024;
const MAX_TEXT: u64 = 256 * 1024;
const MAX_LAYOUT: u64 = 4 * 1024 * 1024;
const MAX_GLYPHS: usize = 24_000;
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(20);
const LAYOUT_TIME_LIMIT: Duration = Duration::from_secs(8);

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TextLine {
    pub(crate) glyphs: Vec<Glyph>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Glyph {
    pub(crate) text: String,
    /// Left, top, right, bottom in sampled image pixels.
    pub(crate) bounds: [u32; 4],
}

#[derive(Clone, Copy)]
enum Format {
    Text,
    Layout,
}

fn engine() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.ancestors()
        .skip(1)
        .take(4)
        .map(|directory| directory.join("native/Tesseract"))
        .find(|directory| {
            [
                "tesseract.exe",
                "tessdata/chi_sim.traineddata",
                "tessdata/eng.traineddata",
            ]
            .iter()
            .all(|file| directory.join(file).is_file())
        })
        .ok_or_else(|| "缺少离线 OCR 引擎或中英文模型，请完整解压 RustView 离线包".into())
}

pub(crate) fn recognize(
    width: u32,
    height: u32,
    gray: &[u8],
    sparse_text: bool,
    cancel: &AtomicBool,
) -> Result<String, String> {
    validate(width, height, gray)?;
    let started = Instant::now();
    let runtime = engine()?;
    let (primary, fallback) = page_modes(sparse_text);
    let text = run(
        &runtime,
        width,
        height,
        gray,
        primary,
        Format::Text,
        cancel,
        started,
    )?;
    if text.is_empty() {
        run(
            &runtime,
            width,
            height,
            gray,
            fallback,
            Format::Text,
            cancel,
            started,
        )
    } else {
        Ok(text)
    }
}

pub(crate) fn recognize_layout(
    width: u32,
    height: u32,
    gray: &[u8],
    sparse_text: bool,
    cancel: &AtomicBool,
) -> Result<Vec<TextLine>, String> {
    validate(width, height, gray)?;
    let started = Instant::now();
    let runtime = engine()?;
    let (primary, fallback) = page_modes(sparse_text);
    let result = run(
        &runtime,
        width,
        height,
        gray,
        primary,
        Format::Layout,
        cancel,
        started,
    )?;
    let lines = parse_layout(&result, width, height, cancel, started)?;
    if lines.is_empty() {
        let result = run(
            &runtime,
            width,
            height,
            gray,
            fallback,
            Format::Layout,
            cancel,
            started,
        )?;
        parse_layout(&result, width, height, cancel, started)
    } else {
        Ok(lines)
    }
}

fn validate(width: u32, height: u32, gray: &[u8]) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || width as usize * height as usize > MAX_PIXELS
        || width as usize * height as usize != gray.len()
    {
        return Err("OCR 图像尺寸超出限额或像素不完整".into());
    }
    Ok(())
}

fn page_modes(sparse_text: bool) -> (&'static str, &'static str) {
    if sparse_text {
        ("11", "3")
    } else {
        ("3", "11")
    }
}

fn interrupted(cancel: &AtomicBool, started: Instant, format: Format) -> Option<&'static str> {
    let limit = match format {
        Format::Text => TIME_LIMIT,
        Format::Layout => LAYOUT_TIME_LIMIT,
    };
    if cancel.load(Ordering::Relaxed) {
        Some("OCR 已取消")
    } else if started.elapsed() >= limit {
        Some("OCR 超时，已停止识别")
    } else {
        None
    }
}

fn run(
    runtime: &std::path::Path,
    width: u32,
    height: u32,
    gray: &[u8],
    segmentation: &str,
    format: Format,
    cancel: &AtomicBool,
    started: Instant,
) -> Result<String, String> {
    if let Some(reason) = interrupted(cancel, started, format) {
        return Err(reason.into());
    }
    let mut command = Command::new(runtime.join("tesseract.exe"));
    command
        // Relative model paths also work when the portable folder has Chinese characters.
        .current_dir(runtime)
        .args([
            "stdin",
            "stdout",
            "--tessdata-dir",
            "tessdata",
            "-l",
            "chi_sim+eng",
            "--psm",
            segmentation,
            "--dpi",
            "150",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let max_output = match format {
        Format::Text => MAX_TEXT,
        Format::Layout => {
            command
                .args(["-c", "tessedit_create_hocr=1", "-c", "hocr_char_boxes=1"])
                .env("OMP_THREAD_LIMIT", "1");
            MAX_LAYOUT
        }
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW, IDLE_PRIORITY_CLASS,
        };
        let priority = match format {
            Format::Text => BELOW_NORMAL_PRIORITY_CLASS,
            Format::Layout => IDLE_PRIORITY_CLASS,
        };
        command.creation_flags(CREATE_NO_WINDOW | priority);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动离线 OCR：{e}"))?;
    #[cfg(windows)]
    let job = match crate::worker::Job::attach_with_limit(&child, MEMORY_LIMIT) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let mut input = child.stdin.take().expect("OCR stdin is piped");
    let output = child.stdout.take().expect("OCR stdout is piped");
    let output = std::thread::scope(|scope| {
        // Send the bounded grayscale image without writing private pixels to disk.
        let write = scope.spawn(move || {
            write!(input, "P5\n{width} {height}\n255\n")?;
            input.write_all(gray)
        });
        let read = scope.spawn(move || {
            let mut bytes = Vec::new();
            output.take(max_output + 1).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        });
        let result = loop {
            if let Some(reason) = interrupted(cancel, started, format) {
                break Err(reason.into());
            }
            match child.try_wait() {
                Ok(Some(exit)) if exit.success() => break Ok(()),
                Ok(Some(_)) => break Err("离线 OCR 进程异常退出或超出内存限额".into()),
                Ok(None) => std::thread::sleep(Duration::from_millis(12)),
                Err(error) => break Err(format!("等待 OCR 进程失败：{error}")),
            }
        };
        // Kill the Windows job before joining readers/writers so cancellation closes pipes.
        #[cfg(windows)]
        drop(job);
        if result.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        result?;
        write
            .join()
            .map_err(|_| "OCR 输入线程中断")?
            .map_err(|e| e.to_string())?;
        read.join()
            .map_err(|_| "OCR 输出线程中断")?
            .map_err(|e| e.to_string())
    })?;
    if let Some(reason) = interrupted(cancel, started, format) {
        return Err(reason.into());
    }
    if output.len() as u64 > max_output {
        return Err("OCR 文本超过输出限额，请缩小识别范围".into());
    }
    String::from_utf8(output)
        .map(|text| match format {
            Format::Text => text.trim().replace("\r\n", "\n"),
            Format::Layout => text,
        })
        .map_err(|e| format!("OCR 返回的文本编码无效：{e}"))
}

fn parse_layout(
    hocr: &str,
    width: u32,
    height: u32,
    cancel: &AtomicBool,
    started: Instant,
) -> Result<Vec<TextLine>, String> {
    let mut lines = Vec::new();
    let mut glyphs = Vec::new();
    let mut new_word = false;
    let mut count = 0;
    // The bundled renderer emits these fixed span prefixes and XML-escapes glyphs.
    for span in hocr
        .split('<')
        .filter_map(|tag| tag.strip_prefix("span class='"))
    {
        if let Some(reason) = interrupted(cancel, started, Format::Layout) {
            return Err(reason.into());
        }
        let Some((class, contents)) = span.split_once('\'') else {
            continue;
        };
        match class {
            "ocr_line" | "ocr_header" | "ocr_textfloat" | "ocr_caption" => {
                if !glyphs.is_empty() {
                    lines.push(TextLine {
                        glyphs: std::mem::take(&mut glyphs),
                    });
                }
                new_word = false;
            }
            "ocrx_word" => new_word = true,
            "ocrx_cinfo" => {
                let Some(glyph) = parse_glyph(contents, width, height) else {
                    continue;
                };
                if new_word {
                    if let Some(space) = glyphs.last().and_then(|prior| word_space(prior, &glyph)) {
                        glyphs.push(space);
                        count += 1;
                    }
                    new_word = false;
                }
                glyphs.push(glyph);
                count += 1;
                if count > MAX_GLYPHS {
                    return Err("OCR 文字过多，请缩小识别范围".into());
                }
            }
            _ => {}
        }
    }
    if !glyphs.is_empty() {
        lines.push(TextLine { glyphs });
    }
    Ok(lines)
}

fn parse_glyph(span: &str, width: u32, height: u32) -> Option<Glyph> {
    let (position, text) = span
        .strip_prefix(" title='x_bboxes ")?
        .split_once("; x_conf ")?;
    let mut coords = position.split_ascii_whitespace().map(str::parse::<u32>);
    let [left, top, right, bottom] = std::array::from_fn(|_| coords.next().and_then(Result::ok));
    let bounds = [left?, top?, right?, bottom?];
    let [left, top, right, bottom] = bounds;
    if left >= right || top >= bottom || right > width || bottom > height {
        return None;
    }
    let text = text.split_once("'>")?.1;
    if text.len() > 64 || text.trim().is_empty() {
        return None;
    }
    let mut parts = text.split('&');
    let mut decoded = parts.next()?.to_owned();
    for part in parts {
        let (entity, tail) = part.split_once(';')?;
        decoded.push(match entity {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" | "#39" => '\'',
            _ => return None,
        });
        decoded.push_str(tail);
    }
    Some(Glyph {
        text: decoded,
        bounds,
    })
}

fn word_space(prior: &Glyph, next: &Glyph) -> Option<Glyph> {
    let [_, top, right, bottom] = prior.bounds;
    let [next_left, next_top, _, next_bottom] = next.bounds;
    let gap = next_left.checked_sub(right)?;
    let height = (bottom - top).min(next_bottom - next_top);
    // Tesseract often marks individual Chinese characters as separate words.
    let has_space = match (prior.text.is_ascii(), next.text.is_ascii()) {
        (true, true) => gap > 0,
        (true, false) | (false, true) => gap * 8 >= height,
        (false, false) => false,
    };
    if !has_space {
        return None;
    }
    Some(Glyph {
        text: " ".into(),
        bounds: [right, top.min(next_top), next_left, bottom.max(next_bottom)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_glyph_bounds_entities_and_reading_order() {
        let hocr = "<span class='ocr_header' id='line_1_1'>\n\
            <span class='ocrx_word' id='word_1_1'>\n\
            <span class='ocrx_cinfo' title='x_bboxes 1 3 12 23; x_conf 99'>你</span>\n\
            <span class='ocrx_word' id='word_1_2'>\n\
            <span class='ocrx_cinfo' title='x_bboxes 17 3 28 23; x_conf 99'>好</span>\n\
            <span class='ocrx_word' id='word_1_3'>\n\
            <span class='ocrx_cinfo' title='x_bboxes 35 3 48 23; x_conf 99'>A</span>\n\
            <span class='ocrx_cinfo' title='x_bboxes 52 3 62 23; x_conf 99'>&amp;</span>\n\
            <span class='ocrx_cinfo' title='x_bboxes 63 3 71 23; x_conf 99'>&#39;</span>\n\
            <span class='ocrx_word' id='word_1_4'>\n\
            <span class='ocrx_cinfo' title='x_bboxes 75 3 81 23; x_conf 99'>B</span>\n\
            <span class='ocr_caption' id='line_1_2'>\n\
            <span class='ocrx_cinfo' title='x_bboxes 3 26 17 38; x_conf 99'>&lt;</span>\n\
            <span class='ocrx_cinfo' title='x_bboxes 17 27 32 39; x_conf 99'>&quot;</span>\n\
            <span class='ocrx_cinfo' title='x_bboxes 32 27 52 39; x_conf 99'>&gt;</span>\n\
            <span class='ocrx_cinfo' title='x_bboxes 52 27 101 39; x_conf 99'>ignored</span>\n\
            <span class='ocrx_cinfo' title='x_bboxes 52 27 61 39; x_conf 99'>&bogus;</span>";
        let lines = parse_layout(hocr, 100, 50, &AtomicBool::new(false), Instant::now()).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0]
                .glyphs
                .iter()
                .map(|g| g.text.as_str())
                .collect::<String>(),
            "你好 A&' B"
        );
        assert_eq!(
            lines[1]
                .glyphs
                .iter()
                .map(|g| g.text.as_str())
                .collect::<String>(),
            "<\">"
        );
        assert_eq!(lines[0].glyphs[0].bounds, [1, 3, 12, 23]);
        assert_eq!(lines[0].glyphs[2].bounds, [28, 3, 35, 23]);
    }

    #[test]
    fn position_output_stops_when_cancelled() {
        let cancel = AtomicBool::new(true);
        assert!(parse_layout("<span class='ocr_line'>", 50, 50, &cancel, Instant::now()).is_err());
    }
}
