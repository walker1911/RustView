use crate::{
    animation::{self, AnimationFrame, RepeatCount},
    worker::{self, Output, Reply, Request},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_FRAME_BYTES: usize = 128 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 2048 * 2048 * 4;
// Only the first packet may carry the bounded 1 MiB edit record.
const MAX_HEADER_BYTES: usize = 1024 * 1024 + 64 * 1024;
const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
const FRAME_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Packet {
    pub output: Output,
    pub animated: bool,
    pub duration_ms: u32,
}

pub enum Event {
    Frame(Packet),
    End,
    Error {
        message: String,
        password_required: bool,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Header {
    Frame {
        reply: Reply,
        duration_ms: u32,
        repeat: RepeatCount,
    },
    Still {
        reply: Reply,
    },
    End,
    Error {
        message: String,
        password_required: bool,
    },
}

fn image_output(width: u32, height: u32, rgba: Vec<u8>, note: String) -> Result<Output, String> {
    let image = image::DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(width, height, rgba).ok_or("帧尺寸和像素数据不一致")?,
    );
    let preview = image
        .thumbnail(width.min(2048), height.min(2048))
        .into_rgba8();
    let reply = Reply {
        width,
        height,
        preview_width: preview.width(),
        preview_height: preview.height(),
        note,
        ..Default::default()
    };
    Ok(Output {
        reply,
        rgba: image.into_rgba8().into_raw(),
        preview: preview.into_raw(),
    })
}

fn write_header(writer: &mut impl Write, header: &Header) -> Result<(), String> {
    let bytes = serde_json::to_vec(header).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_HEADER_BYTES {
        return Err("动画结果头部超过记录预算".into());
    }
    writer
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn write_output(
    writer: &mut impl Write,
    header: Header,
    rgba: &[u8],
    preview: &[u8],
) -> Result<(), String> {
    write_header(writer, &header)?;
    writer.write_all(rgba).map_err(|e| e.to_string())?;
    writer.write_all(preview).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

fn line(reader: &mut impl BufRead, limit: u64) -> Result<String, String> {
    let mut text = String::new();
    reader
        .take(limit + 1)
        .read_line(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > limit {
        return Err("动画控制请求过长".into());
    }
    Ok(text)
}

fn failure(message: impl std::fmt::Display) -> (String, bool) {
    (message.to_string(), false)
}

fn load_settings(request: &Request) -> (bool, bool) {
    match request {
        Request::Load { full_raw, .. } => (*full_raw, true),
        Request::ColorPreview { source, managed } => (load_settings(source).0, *managed),
        Request::Unedited { source } => load_settings(source),
        _ => (false, true),
    }
}

fn run_worker(
    directory: &Path,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<(), (String, bool)> {
    let request: Request = serde_json::from_str(&line(input, MAX_REQUEST_BYTES).map_err(failure)?)
        .map_err(|_| failure("无效的动画读取请求"))?;
    let (path, mut record) =
        worker::prepare_recorded_input(&request, directory).map_err(|reply| {
            (
                reply.error.unwrap_or_else(|| "无法准备图像".into()),
                reply.password_required,
            )
        })?;
    let (full_raw, managed) = load_settings(&request);
    let Some(mut stream) = animation::open(&path, MAX_FRAME_BYTES).map_err(failure)? else {
        let decode = if managed {
            crate::imaging::decode
        } else {
            crate::imaging::decode_unmanaged
        };
        let image =
            decode(&path, 200_000_000, worker::MAX_IMAGE_BYTES, full_raw).map_err(failure)?;
        let mut image =
            image_output(image.width, image.height, image.rgba, image.note).map_err(failure)?;
        image.reply.editing_record = record.take();
        write_output(
            output,
            Header::Still { reply: image.reply },
            &image.rgba,
            &image.preview,
        )
        .map_err(failure)?;
        return Ok(());
    };
    stream.set_color_management(managed);
    let repeat = stream.repeat();
    let mut plays_completed = 0_u32;
    let mut frame_number = 0_u32;
    loop {
        let frame: AnimationFrame = loop {
            match stream.next_frame().map_err(failure)? {
                Some(frame) => break frame,
                None => {
                    plays_completed = plays_completed.saturating_add(1);
                    if matches!(repeat, RepeatCount::Finite(total) if plays_completed >= total) {
                        write_header(output, &Header::End).map_err(failure)?;
                        output.flush().map_err(failure)?;
                        return Ok(());
                    }
                    stream.reset().map_err(failure)?;
                    frame_number = 0;
                }
            }
        };
        frame_number = frame_number.saturating_add(1);
        let duration_ms = frame.duration_ms;
        let note = match repeat {
            RepeatCount::Infinite => format!(
                "动画 · 第 {frame_number} 帧 · 第 {} 轮（循环）",
                plays_completed + 1
            ),
            RepeatCount::Finite(total) => format!(
                "动画 · 第 {frame_number} 帧 · 第 {}/{total} 轮",
                plays_completed + 1
            ),
        };
        {
            let mut image =
                image_output(frame.width, frame.height, frame.rgba, note).map_err(failure)?;
            image.reply.editing_record = record.take();
            write_output(
                output,
                Header::Frame {
                    reply: image.reply,
                    duration_ms,
                    repeat,
                },
                &image.rgba,
                &image.preview,
            )
            .map_err(failure)?;
        }
        // The decoder advances only after the client requests its single next frame.
        let command = line(input, 32).map_err(failure)?;
        if command.is_empty() || command.trim() == "stop" {
            return Ok(());
        }
        if command.trim() != "next" {
            return Err(failure("无效的动画控制命令"));
        }
    }
}

pub fn worker_main() {
    let Some(directory) = std::env::args_os().nth(2).map(PathBuf::from) else {
        return;
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = BufWriter::new(stdout.lock());
    if let Err((message, password_required)) = run_worker(&directory, &mut input, &mut output) {
        let _ = write_header(
            &mut output,
            &Header::Error {
                message,
                password_required,
            },
        );
        let _ = output.flush();
    }
}

fn pixel_count(width: u32, height: u32, limit: usize) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| width > 0 && height > 0 && *n <= limit)
        .ok_or_else(|| "后台帧尺寸无效或超过内存预算".into())
}

fn read_bytes(reader: &mut impl Read, length: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|e| format!("无法分配帧缓冲：{e}"))?;
    bytes.resize(length, 0);
    reader
        .read_exact(&mut bytes)
        .map_err(|_| "动画数据流中断，后台进程可能超限或解码失败".to_owned())?;
    Ok(bytes)
}

fn read_event(reader: &mut impl Read) -> Result<Event, String> {
    let mut length = [0; 4];
    reader
        .read_exact(&mut length)
        .map_err(|_| "动画结果流已中断".to_owned())?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_HEADER_BYTES {
        return Err("动画结果头部长度无效".into());
    }
    let header: Header = serde_json::from_slice(&read_bytes(reader, length)?)
        .map_err(|e| format!("动画结果头部格式无效：{e}"))?;
    let (reply, animated, duration_ms) = match header {
        Header::Frame {
            reply, duration_ms, ..
        } => (reply, true, duration_ms.max(10)),
        Header::Still { reply } => (reply, false, 0),
        Header::End => return Ok(Event::End),
        Header::Error {
            message,
            password_required,
        } => {
            return Ok(Event::Error {
                message,
                password_required,
            })
        }
    };
    let rgba_len = pixel_count(
        reply.width,
        reply.height,
        if animated {
            MAX_FRAME_BYTES
        } else {
            worker::MAX_IMAGE_BYTES
        },
    )?;
    if reply.preview_width > 2048 || reply.preview_height > 2048 {
        return Err("动画预览尺寸超过限制".into());
    }
    let preview_len = pixel_count(reply.preview_width, reply.preview_height, MAX_PREVIEW_BYTES)?;
    let rgba = read_bytes(reader, rgba_len)?;
    let preview = read_bytes(reader, preview_len)?;
    Ok(Event::Frame(Packet {
        output: Output {
            reply,
            rgba,
            preview,
        },
        animated,
        duration_ms,
    }))
}

struct Shared {
    child: Mutex<Option<Child>>,
    deadline: Mutex<Option<Instant>>,
    cancelled: AtomicBool,
    timed_out: AtomicBool,
    terminal: AtomicBool,
    finished: AtomicBool,
}

fn kill_shared(shared: &Shared) {
    if let Ok(mut child) = shared.child.try_lock() {
        if let Some(child) = child.as_mut() {
            let _ = child.kill();
        }
    }
}

struct Resources {
    shared: Arc<Shared>,
    context: egui::Context,
    directory: Option<tempfile::TempDir>,
    #[cfg(windows)]
    job: Option<worker::Job>,
}

impl Drop for Resources {
    fn drop(&mut self) {
        let child = self
            .shared
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        #[cfg(windows)]
        drop(self.job.take());
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        drop(self.directory.take());
        self.shared.finished.store(true, Ordering::Release);
        self.context.request_repaint();
    }
}

fn send_event(
    sender: &SyncSender<Event>,
    event: Event,
    shared: &Shared,
    context: &egui::Context,
) -> bool {
    if shared.cancelled.load(Ordering::Acquire) {
        return false;
    }
    let sent = sender.try_send(event).is_ok();
    if sent {
        context.request_repaint();
    }
    sent
}

fn client_loop(
    request: Request,
    commands: Receiver<()>,
    events: &SyncSender<Event>,
    resources: &mut Resources,
) -> Result<(), String> {
    if resources.shared.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let directory = tempfile::Builder::new()
        .prefix("rustview-animation-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let path = directory.path().to_owned();
    resources.directory = Some(directory);
    let mut json = serde_json::to_vec(&request).map_err(|_| "无法编码动画请求".to_owned())?;
    if json.len() as u64 > MAX_REQUEST_BYTES {
        return Err("动画读取请求过长".into());
    }
    json.push(b'\n');
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("--animation-worker")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动动画读取进程：{e}"))?;
    let input = child.stdin.take().ok_or("无法打开动画输入管道")?;
    let output = child.stdout.take().ok_or("无法打开动画输出管道")?;
    // Save the child before any fallible setup so cancellation and cleanup can always kill it.
    *resources
        .shared
        .child
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(child);
    #[cfg(windows)]
    {
        let child = resources
            .shared
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        resources.job = Some(worker::Job::attach(child.as_ref().unwrap())?);
    }
    if resources.shared.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut input = BufWriter::new(input);
    let mut output = BufReader::new(output);
    *resources
        .shared
        .deadline
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    input
        .write_all(&json)
        .and_then(|_| input.flush())
        .map_err(|_| "动画请求管道已关闭".to_owned())?;
    drop(json);
    loop {
        if resources.shared.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let event = read_event(&mut output)?;
        *resources
            .shared
            .deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        let terminal = !matches!(&event, Event::Frame(Packet { animated: true, .. }));
        if terminal {
            resources.shared.terminal.store(true, Ordering::Release);
        }
        if !send_event(events, event, &resources.shared, &resources.context) || terminal {
            return Ok(());
        }
        if commands.recv().is_err() || resources.shared.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        *resources
            .shared
            .deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        input
            .write_all(b"next\n")
            .and_then(|_| input.flush())
            .map_err(|_| "动画控制管道已关闭".to_owned())?;
    }
}

pub struct Session {
    events: Receiver<Event>,
    commands: SyncSender<()>,
    shared: Arc<Shared>,
    in_flight: AtomicBool,
}

impl Session {
    pub fn start(request: Request, context: egui::Context) -> Self {
        let shared = Arc::new(Shared {
            child: Mutex::new(None),
            deadline: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            terminal: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        });
        let (command_sender, commands) = mpsc::sync_channel(1);
        let (events, event_receiver) = mpsc::sync_channel(1);
        let watchdog = shared.clone();
        std::thread::spawn(move || {
            while !watchdog.finished.load(Ordering::Acquire) {
                if watchdog.cancelled.load(Ordering::Acquire) {
                    kill_shared(&watchdog);
                }
                let timed_out = watchdog
                    .deadline
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_some_and(|started| started.elapsed() > FRAME_TIMEOUT);
                if timed_out {
                    watchdog.timed_out.store(true, Ordering::Release);
                    kill_shared(&watchdog);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        });
        let thread_shared = shared.clone();
        std::thread::spawn(move || {
            let mut resources = Resources {
                shared: thread_shared.clone(),
                context,
                directory: None,
                #[cfg(windows)]
                job: None,
            };
            if let Err(message) = client_loop(request, commands, &events, &mut resources) {
                thread_shared.terminal.store(true, Ordering::Release);
                let message = if thread_shared.timed_out.load(Ordering::Acquire) {
                    "动画帧读取超时，后台任务已停止".into()
                } else {
                    message
                };
                send_event(
                    &events,
                    Event::Error {
                        message,
                        password_required: false,
                    },
                    &thread_shared,
                    &resources.context,
                );
            }
            // Resources kills/waits the process tree and removes its temporary archive input.
        });
        Self {
            events: event_receiver,
            commands: command_sender,
            shared,
            in_flight: AtomicBool::new(true),
        }
    }

    pub fn next(&self) -> bool {
        if self.shared.cancelled.load(Ordering::Acquire)
            || self.shared.terminal.load(Ordering::Acquire)
            || self.shared.finished.load(Ordering::Acquire)
        {
            return false;
        }
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        if self.commands.try_send(()).is_err() {
            self.in_flight.store(false, Ordering::Release);
            return false;
        }
        true
    }

    pub fn try_recv(&self) -> Option<Event> {
        let event = self.events.try_recv().ok()?;
        self.in_flight.store(false, Ordering::Release);
        Some(event)
    }

    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
        kill_shared(&self.shared);
        let _ = self.commands.try_send(());
    }

    pub fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub fn probe_main() {
    let arguments: Vec<String> = std::env::args().collect();
    let Some(path) = arguments.get(2).map(PathBuf::from) else {
        eprintln!("Usage: RustView --animation-probe PATH [--entry NAME] [--frames N] [--password-stdin] [--cancel-after-ms N]");
        std::process::exit(2);
    };
    let option = |name: &str| {
        arguments
            .iter()
            .position(|value| value == name)
            .and_then(|index| arguments.get(index + 1))
    };
    let frames = option("--frames")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(12)
        .clamp(1, 10_000);
    let cancel_after = option("--cancel-after-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis);
    let entry = option("--entry").cloned();
    let password = if arguments.iter().any(|value| value == "--password-stdin") {
        let mut password = String::new();
        let _ = std::io::stdin().take(16384).read_to_string(&mut password);
        Some(password.trim_end_matches(['\r', '\n']).to_owned())
    } else {
        None
    };
    let mut request = Request::Load {
        path,
        entry,
        password,
        full_raw: false,
    };
    if arguments.iter().any(|value| value == "--unmanaged") {
        request = Request::ColorPreview {
            source: Box::new(request),
            managed: false,
        };
    }
    let session = Session::start(request, egui::Context::default());
    let started = Instant::now();
    let mut summaries = Vec::new();
    let mut error = None;
    let mut password_required = false;
    let reason = loop {
        if cancel_after.is_some_and(|limit| started.elapsed() >= limit) {
            break "cancelled";
        }
        let mut event = session.try_recv();
        let finished = session.finished();
        // Completion can race the first channel read; cleanup publishes finished after sending.
        if event.is_none() && finished {
            event = session.try_recv();
        }
        if let Some(event) = event {
            match event {
                Event::Frame(packet) => {
                    let checksum = |bytes: &[u8]| {
                        let value = bytes.iter().fold(0xcbf29ce484222325_u64, |value, byte| {
                            (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
                        });
                        format!("{value:016x}")
                    };
                    summaries.push(serde_json::json!({
                        "width": packet.output.reply.width, "height": packet.output.reply.height,
                        "preview_width": packet.output.reply.preview_width, "preview_height": packet.output.reply.preview_height,
                        "animated": packet.animated, "duration_ms": packet.duration_ms,
                        "rgba_hash": checksum(&packet.output.rgba), "preview_hash": checksum(&packet.output.preview),
                        "note": packet.output.reply.note,
                    }));
                    if !packet.animated {
                        break "still";
                    }
                    if summaries.len() >= frames {
                        break "frame_limit";
                    }
                    session.next();
                }
                Event::End => break "end",
                Event::Error {
                    message,
                    password_required: needed,
                } => {
                    error = Some(message);
                    password_required = needed;
                    break "error";
                }
            }
        } else if finished {
            error = Some("动画会话未返回结束状态".to_owned());
            break "error";
        }
        if started.elapsed() > Duration::from_secs(240) {
            error = Some("动画验收总超时".to_owned());
            break "timeout";
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let cleanup = Instant::now();
    session.cancel();
    while !session.finished() && cleanup.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let finished = session.finished();
    let report = serde_json::json!({
        "reason": reason, "frame_count": summaries.len(), "frames": summaries,
        "error": error, "password_required": password_required, "finished": finished,
        "cleanup_ms": cleanup.elapsed().as_millis(), "elapsed_ms": started.elapsed().as_millis(),
    });
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &report);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
    std::process::exit(if error.is_none() && finished { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn request(path: &Path) -> Vec<u8> {
        let mut request = serde_json::to_vec(&Request::Load {
            path: path.to_owned(),
            entry: None,
            password: None,
            full_raw: false,
        })
        .unwrap();
        request.extend_from_slice(b"\nnext\nnext\nnext\nnext\nnext\nnext\n");
        request
    }

    #[test]
    fn worker_streams_exact_total_plays_and_clamped_delays() {
        let directory = tempfile::tempdir().unwrap();
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/animation-previous.gif");
        let mut output = Vec::new();
        assert!(run_worker(
            directory.path(),
            &mut Cursor::new(request(&path)),
            &mut output
        )
        .is_ok());
        let mut stream = Cursor::new(output);
        for expected in [30, 50, 10, 30, 50, 10] {
            let Event::Frame(packet) = read_event(&mut stream).unwrap() else {
                panic!("expected frame")
            };
            assert!(packet.animated);
            assert_eq!(packet.duration_ms, expected);
            assert_eq!(packet.output.rgba.len(), 32);
            assert_eq!(packet.output.preview.len(), 32);
        }
        assert!(matches!(read_event(&mut stream).unwrap(), Event::End));
    }

    #[test]
    fn worker_still_preserves_pixels_and_does_not_upscale_preview() {
        let directory = tempfile::tempdir().unwrap();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test.webp");
        let mut output = Vec::new();
        assert!(run_worker(
            directory.path(),
            &mut Cursor::new(request(&path)),
            &mut output
        )
        .is_ok());
        let Event::Frame(packet) = read_event(&mut Cursor::new(output)).unwrap() else {
            panic!("expected still")
        };
        assert!(!packet.animated);
        assert_eq!(packet.output.reply.width, 2);
        assert_eq!(packet.output.reply.preview_width, 2);
        assert_eq!(packet.output.rgba.len(), 16);
    }

    #[test]
    fn frame_previews_preserve_wide_aspect_ratio_and_do_not_upscale() {
        for (width, height, expected) in [(4096, 512, (2048, 256)), (3, 2, (3, 2))] {
            let rgba = vec![255; width as usize * height as usize * 4];
            let output = image_output(width, height, rgba, String::new()).unwrap();
            assert_eq!(
                (output.reply.preview_width, output.reply.preview_height),
                expected
            );
            assert_eq!(output.rgba.len(), width as usize * height as usize * 4);
            assert_eq!(
                output.preview.len(),
                expected.0 as usize * expected.1 as usize * 4
            );
        }
    }
    #[test]
    fn saved_animation_recipe_renders_once_and_only_first_packet_has_record() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("animation.gif");
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/animation-previous.gif"),
            &source,
        )
        .unwrap();
        let mut recipe = crate::recipes::Recipe::default();
        recipe.edits.push(crate::editing::Edit::Resize {
            width: 2,
            height: 4,
        });
        let record = crate::recipes::EditRecord::capture(&source, None, recipe).unwrap();
        let path =
            crate::recipes::record_path(&crate::recipes::data_root().unwrap(), &source, None)
                .unwrap();
        record.save(&path).unwrap();
        let mut output = Vec::new();
        run_worker(
            directory.path(),
            &mut Cursor::new(request(&source)),
            &mut output,
        )
        .unwrap();
        let mut stream = Cursor::new(output);
        for (index, delay) in [30, 50, 10, 30, 50, 10].into_iter().enumerate() {
            let Event::Frame(packet) = read_event(&mut stream).unwrap() else {
                panic!("expected animation");
            };
            assert_eq!(
                (packet.output.reply.width, packet.output.reply.height),
                (2, 4)
            );
            assert_eq!(packet.duration_ms, delay);
            assert_eq!(packet.output.reply.editing_record.is_some(), index == 0);
        }
        assert!(matches!(read_event(&mut stream).unwrap(), Event::End));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_oversized_wire_headers_and_frames_before_reading_pixels() {
        assert!(read_event(&mut Cursor::new(
            ((MAX_HEADER_BYTES + 1) as u32).to_le_bytes()
        ))
        .is_err());
        let reply = Reply {
            width: 20_000,
            height: 20_000,
            preview_width: 1,
            preview_height: 1,
            ..Default::default()
        };
        let mut bytes = Vec::new();
        write_header(
            &mut bytes,
            &Header::Frame {
                reply,
                duration_ms: 20,
                repeat: RepeatCount::Infinite,
            },
        )
        .unwrap();
        assert!(read_event(&mut Cursor::new(bytes)).is_err());
    }

    #[test]
    fn password_error_wire_does_not_require_pixel_payload() {
        let mut bytes = Vec::new();
        write_header(
            &mut bytes,
            &Header::Error {
                message: "密码不正确".into(),
                password_required: true,
            },
        )
        .unwrap();
        assert!(matches!(
            read_event(&mut Cursor::new(bytes)).unwrap(),
            Event::Error {
                password_required: true,
                ..
            }
        ));
    }
}
