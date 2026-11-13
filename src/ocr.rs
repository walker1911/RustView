//! Run bundled Tesseract only when the user requests text recognition.
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
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(20);

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
    cancel: &AtomicBool,
) -> Result<String, String> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || width as usize * height as usize > MAX_PIXELS
        || width as usize * height as usize != gray.len()
    {
        return Err("OCR 图像尺寸超出限额或像素不完整".into());
    }
    let started = Instant::now();
    let runtime = engine()?;
    let text = run(&runtime, width, height, gray, "3", cancel, started)?;
    if text.is_empty() {
        // Automatic page layout may reject isolated lines in a tight selection.
        run(&runtime, width, height, gray, "11", cancel, started)
    } else {
        Ok(text)
    }
}

fn interrupted(cancel: &AtomicBool, started: Instant) -> Option<&'static str> {
    if cancel.load(Ordering::Relaxed) {
        Some("OCR 已取消")
    } else if started.elapsed() >= TIME_LIMIT {
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
    cancel: &AtomicBool,
    started: Instant,
) -> Result<String, String> {
    if let Some(reason) = interrupted(cancel, started) {
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW,
        };
        command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
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
            output.take(MAX_TEXT + 1).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        });
        let result = loop {
            if let Some(reason) = interrupted(cancel, started) {
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
    if let Some(reason) = interrupted(cancel, started) {
        return Err(reason.into());
    }
    if output.len() as u64 > MAX_TEXT {
        return Err("OCR 文本超过输出限额，请缩小识别范围".into());
    }
    String::from_utf8(output)
        .map(|text| text.trim().replace("\r\n", "\n"))
        .map_err(|e| format!("OCR 返回的文本编码无效：{e}"))
}
