use crate::{pyramid::Source, worker::PyramidInfo};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
};

pub(crate) type Key = (usize, u32, u32);
pub(crate) const IN_FLIGHT: usize = 4;

pub(crate) enum TileReply {
    Pixels(Vec<u8>),
    Unreadable,
    Superseded,
}

pub(crate) struct Tiles {
    pub(crate) requests: SyncSender<Option<Key>>,
    pub(crate) replies: Receiver<Result<TileReply, String>>,
    pub(crate) wanted: Arc<Mutex<Vec<Key>>>,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl Tiles {
    pub(crate) fn start(
        path: PathBuf,
        managed: bool,
        info: PyramidInfo,
        ctx: egui::Context,
    ) -> Result<Self, String> {
        let (requests, keys) = mpsc::sync_channel(IN_FLIGHT);
        let (results, replies) = mpsc::sync_channel(IN_FLIGHT);
        let wanted = Arc::new(Mutex::new(Vec::<Key>::new()));
        let worker_wanted = wanted.clone();
        let child = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_child = child.clone();
        let worker_cancelled = cancelled.clone();
        let finished = Arc::new(AtomicBool::new(false));
        let worker_finished = finished.clone();
        std::thread::Builder::new()
            .name("rustview-tiff-tiles".into())
            .spawn(move || {
                let result = (|| -> Result<(), String> {
                    if worker_cancelled.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    let mut command =
                        Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
                    command
                        .arg("--pyramid-tiles")
                        .arg(path)
                        .arg(if managed { "managed" } else { "unmanaged" })
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
                    let mut process = command.spawn().map_err(|e| e.to_string())?;
                    #[cfg(windows)]
                    let _job =
                        match crate::worker::Job::attach_with_limit(&process, 512 * 1024 * 1024) {
                            Ok(job) => job,
                            Err(error) => {
                                let _ = process.kill();
                                let _ = process.wait();
                                return Err(error);
                            }
                        };
                    let mut input = process.stdin.take().expect("tile worker stdin is piped");
                    let mut output =
                        BufReader::new(process.stdout.take().expect("tile worker stdout is piped"));
                    let mut slot = worker_child.lock().unwrap();
                    *slot = Some(process);
                    drop(slot);
                    if worker_cancelled.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    // The decoder waits for this line before opening the TIFF inside the job.
                    send_line(&mut input, &info)?;
                    let mut status = [0];
                    output.read_exact(&mut status).map_err(|e| e.to_string())?;
                    if status != [1] {
                        return Err("TIFF 文件已更改或无法重新打开".into());
                    }
                    while let Ok(Some(key)) = keys.recv() {
                        if worker_cancelled.load(Ordering::Relaxed) {
                            break;
                        }
                        if !worker_wanted.lock().unwrap().contains(&key) {
                            if results.send(Ok(TileReply::Superseded)).is_err() {
                                break;
                            }
                            ctx.request_repaint();
                            continue;
                        }
                        let (width, height) =
                            tile_size(&info, key).ok_or("无效的 TIFF 分块索引")?;
                        send_line(&mut input, &key)?;
                        output.read_exact(&mut status).map_err(|e| e.to_string())?;
                        let tile = match status[0] {
                            0 => TileReply::Unreadable,
                            1 => {
                                let mut rgba = vec![0; width as usize * height as usize * 4];
                                output.read_exact(&mut rgba).map_err(|e| e.to_string())?;
                                TileReply::Pixels(rgba)
                            }
                            _ => return Err("无效的 TIFF 分块回复".into()),
                        };
                        if results.send(Ok(tile)).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                    Ok(())
                })();
                let process = worker_child.lock().unwrap().take();
                if let Some(mut process) = process {
                    let _ = process.kill();
                    let _ = process.wait();
                }
                if let Err(error) = result {
                    if !worker_cancelled.load(Ordering::Relaxed) {
                        let _ = results.try_send(Err(format!("TIFF 细节读取失败：{error}")));
                    }
                }
                drop(results);
                worker_finished.store(true, Ordering::Release);
                ctx.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            requests,
            replies,
            wanted,
            child,
            cancelled,
            finished,
        })
    }

    pub(crate) fn cancel(&self) {
        if self.cancelled.swap(true, Ordering::Relaxed) {
            return;
        }
        let _ = self.requests.try_send(None);
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }

    pub(crate) fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

impl Drop for Tiles {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(crate) fn tile_size(info: &PyramidInfo, (level, x, y): Key) -> Option<(u32, u32)> {
    let level = info.levels.get(level)?;
    if !(1..=2048).contains(&level.tile_width) || !(1..=2048).contains(&level.tile_height) {
        return None;
    }
    let w = level.width.checked_sub(x.checked_mul(level.tile_width)?)?;
    let h = level
        .height
        .checked_sub(y.checked_mul(level.tile_height)?)?;
    (w != 0 && h != 0).then_some((w.min(level.tile_width), h.min(level.tile_height)))
}

fn send_line(out: &mut impl Write, value: &impl serde::Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *out, value).map_err(|e| e.to_string())?;
    out.write_all(b"\n").map_err(|e| e.to_string())
}

fn read_line<T: serde::de::DeserializeOwned>(input: &mut impl BufRead, limit: u64) -> Option<T> {
    let mut line = Vec::new();
    input.take(limit).read_until(b'\n', &mut line).ok()?;
    if line.last() != Some(&b'\n') {
        return None;
    }
    serde_json::from_slice(&line).ok()
}

pub(crate) fn worker_main() {
    let mut args = std::env::args_os().skip(2);
    let Some(path) = args.next().map(PathBuf::from) else {
        return;
    };
    let managed = args.next().is_some_and(|arg| arg == "managed");
    let mut input = std::io::stdin().lock();
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    let Some(info) = read_line::<PyramidInfo>(&mut input, 8192) else {
        return;
    };
    let Some(mut source) = Source::open(&path).ok().flatten().filter(|source| {
        source.levels == info.levels
            && source.orientation.to_exif() == info.orientation
            && source.stamp().is_ok_and(|stamp| stamp == info.stamp)
    }) else {
        let _ = output.write_all(&[0]).and_then(|_| output.flush());
        return;
    };
    source.set_color_managed(managed);
    if output.write_all(&[1]).and_then(|_| output.flush()).is_err() {
        return;
    }
    while let Some(key) = read_line::<Key>(&mut input, 128) {
        let (level, x, y) = key;
        let data = tile_size(&info, key).and_then(|(w, h)| {
            source
                .read_tile(level, x, y)
                .ok()
                .filter(|rgba| rgba.len() == w as usize * h as usize * 4)
        });
        let result = if let Some(data) = data {
            output.write_all(&[1]).and_then(|_| output.write_all(&data))
        } else {
            output.write_all(&[0])
        };
        if result.and_then(|_| output.flush()).is_err() {
            break;
        }
    }
}
