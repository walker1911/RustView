//! Real Viewer queue/worker regression without a native window or filmstrip jobs.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::time::Duration;

#[derive(Deserialize)]
struct Input {
    archive: PathBuf,
    password: Option<String>,
    seed_entry: String,
    large_entry: String,
    final_entry: String,
    temp_root: Option<PathBuf>,
    #[serde(default = "default_delay")]
    switch_delay_ms: u64,
}
fn default_delay() -> u64 {
    250
}

#[derive(Default, Serialize)]
struct Stats {
    frames: usize,
    welcome_frames_after_seed: usize,
    retired_stale_generations: usize,
    cancel_requests: usize,
    navigation_actions: usize,
    partial_bytes_before_first_switch: u64,
    partial_bytes_before_window_close: u64,
    maximum_cache_bytes: usize,
}

fn digest(bytes: &[u8]) -> String {
    format!(
        "{:016x}",
        bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| (hash
            ^ u64::from(*byte))
        .wrapping_mul(0x100000001b3))
    )
}

fn frame(
    viewer: &mut Viewer,
    ctx: &egui::Context,
    stats: &mut Stats,
    selected: usize,
    seeded: bool,
) -> Result<(), String> {
    let previous = viewer.running.as_ref().map(|r| r.id);
    let serial = viewer.serial;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(960.0, 640.0))),
            ..Default::default()
        },
        |ctx| {
            viewer.poll(ctx);
            egui::CentralPanel::default().show(ctx, |ui| viewer.draw_image(ui, ctx));
        },
    );
    stats.frames += 1;
    stats.maximum_cache_bytes = stats.maximum_cache_bytes.max(viewer.cache.bytes);
    if previous.is_some_and(|id| id != serial) && previous != viewer.running.as_ref().map(|r| r.id)
    {
        stats.retired_stale_generations += 1;
    }
    if seeded
        && output.shapes.iter().any(|shape| {
            matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "RustView")
        })
    {
        stats.welcome_frames_after_seed += 1;
    }
    if viewer.selected != Some(selected) {
        return Err("过期任务覆盖了最后选择的导航目标".into());
    }
    if viewer.password_dialog {
        return Err("已提供的会话密码未完成导航读取".into());
    }
    if let Some(error) = &viewer.error {
        return Err(error.clone());
    }
    Ok(())
}

fn pump_until(
    viewer: &mut Viewer,
    ctx: &egui::Context,
    stats: &mut Stats,
    selected: usize,
    seeded: bool,
    done: impl Fn(&Viewer) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(150);
    while !done(viewer) {
        frame(viewer, ctx, stats, selected, seeded)?;
        if Instant::now() > deadline {
            return Err("导航回归等待目标超时".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn partial_size(root: &Path) -> u64 {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|item| item.file_name().to_string_lossy().starts_with("rustview-"))
        .flat_map(|item| {
            std::fs::read_dir(item.path())
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
        })
        .filter(|item| item.file_name().to_string_lossy().starts_with("input."))
        // Windows directory enumeration can cache length until the writer closes.
        // Query an opened handle to observe the spool's current EOF instead.
        .filter_map(|item| {
            std::fs::File::open(item.path())
                .and_then(|file| file.metadata())
                .ok()
                .map(|metadata| metadata.len())
        })
        .max()
        .unwrap_or(0)
}

fn wait_for_partial(
    viewer: &mut Viewer,
    ctx: &egui::Context,
    stats: &mut Stats,
    root: &Path,
    selected: usize,
    advertised: u64,
) -> Result<u64, String> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        frame(viewer, ctx, stats, selected, true)?;
        let partial = partial_size(root);
        if partial >= 64 * 1024 && partial < advertised && viewer.running.is_some() {
            return Ok(partial);
        }
        if viewer.current_key == Some((selected, false)) || Instant::now() > deadline {
            return Err("未捕获到大条目实际提取中途，不能作为取消回归".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn navigate(viewer: &mut Viewer, ctx: &egui::Context, stats: &mut Stats, target: usize) {
    let position = viewer
        .extra
        .visible
        .iter()
        .position(|i| Some(*i) == viewer.selected)
        .unwrap();
    let next = viewer
        .extra
        .visible
        .iter()
        .position(|i| *i == target)
        .unwrap();
    let cancel = viewer.running.as_ref().map(|r| r.cancel.clone());
    let was_canceled = cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed));
    viewer.perform(
        if next > position {
            Action::Next
        } else {
            Action::Previous
        },
        ctx,
    );
    stats.navigation_actions += 1;
    if !was_canceled && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        stats.cancel_requests += 1;
    }
}

fn exercise(
    input: &Input,
    root: &Path,
    viewer: &mut Viewer,
    ctx: &egui::Context,
    stats: &mut Stats,
) -> Result<serde_json::Value, String> {
    let listing = worker::execute(
        Request::List {
            path: input.archive.clone(),
            password: input.password.clone(),
        },
        Arc::new(AtomicBool::new(false)),
    )?;
    if let Some(error) = listing.reply.error {
        return Err(error);
    }
    viewer.entries = listing.reply.entries.ok_or("导航回归缺少压缩包索引")?;
    let index = |name: &str| {
        viewer
            .entries
            .iter()
            .position(|e| e.name == name)
            .ok_or("缺少指定的导航测试条目")
    };
    let seed = index(&input.seed_entry)?;
    let large = index(&input.large_entry)?;
    let last = index(&input.final_entry)?;
    if seed == large || large == last {
        return Err("大条目与seed/final必须不同".into());
    }
    let advertised = viewer.entries[large].size;
    if advertised <= worker::MAX_INPUT_BYTES {
        return Err("导航回归要求大于512 MiB的包内PSD/PSB".into());
    }
    let reference = worker::execute(
        Request::Load {
            path: input.archive.clone(),
            entry: Some(input.final_entry.clone()),
            password: input.password.clone(),
            full_raw: false,
        },
        Arc::new(AtomicBool::new(false)),
    )?;
    if let Some(error) = &reference.reply.error {
        return Err(error.clone());
    }
    let expected = digest(&reference.rgba);
    let dimensions = [reference.reply.width, reference.reply.height];
    drop(reference);
    viewer.source = Some(input.archive.clone());
    viewer.password = input.password.clone();
    viewer.extra.visible = if seed == last {
        vec![seed, large]
    } else {
        vec![seed, large, last]
    };
    viewer.load_index(seed, false);
    pump_until(viewer, ctx, stats, seed, false, |v| {
        v.current_key == Some((seed, false)) && v.running.is_none() && v.pending.is_none()
    })?;
    frame(viewer, ctx, stats, seed, false)?;
    if !viewer.extra.presentation.has_frame() {
        return Err("真实seed图未进入连续显示层".into());
    }
    navigate(viewer, ctx, stats, large);
    stats.partial_bytes_before_first_switch =
        wait_for_partial(viewer, ctx, stats, root, large, advertised)?;
    // Cancel the first generation while its spool is demonstrably partial.
    // Following actions run at a fixed interval through the real GUI handler.
    for target in [last, large, last, large, last] {
        navigate(viewer, ctx, stats, target);
        let until = Instant::now() + Duration::from_millis(input.switch_delay_ms);
        while Instant::now() < until {
            frame(viewer, ctx, stats, target, true)?;
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pump_until(viewer, ctx, stats, last, true, |v| {
        v.current_key == Some((last, false)) && v.running.is_none() && v.pending.is_none()
    })?;
    let image = viewer.current.as_ref().ok_or("最后目标没有图像")?;
    let actual = digest(&image.rgba);
    if expected != actual
        || dimensions != [image.width, image.height]
        || stats.welcome_frames_after_seed != 0
        || stats.retired_stale_generations == 0
        || stats.cancel_requests == 0
        || stats.maximum_cache_bytes > viewer.cache.budget
    {
        return Err("导航目标/像素/连续显示/任务退休/内存预算断言失败".into());
    }
    Ok(
        serde_json::json!({"final_target_matched":true,"dimensions":dimensions,
        "pixel_digest_algorithm":"fnv1a64","expected_pixel_digest":expected,"actual_pixel_digest":actual}),
    )
}

pub fn probe_main() {
    let result = (|| -> Result<serde_json::Value, String> {
        let mut json = Vec::new();
        std::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut json)
            .map_err(|e| e.to_string())?;
        if json.len() > 1024 * 1024 {
            return Err("导航测试请求超过1 MiB".into());
        }
        let input: Input = serde_json::from_slice(&json).map_err(|_| "无效的导航测试请求")?;
        if !(1..=1000).contains(&input.switch_delay_ms) {
            return Err("翻图间隔必须在1–1000毫秒".into());
        }
        let root = tempfile::Builder::new()
            .prefix("rustview-navigation-")
            .tempdir_in(input.temp_root.as_deref().unwrap_or(&std::env::temp_dir()))
            .map_err(|e| e.to_string())?;
        println!(
            "{}",
            serde_json::json!({"event":"navigation_probe_temp","temp_root":root.path()})
        );
        let _ = std::io::stdout().flush();
        let previous_temp = std::env::var_os("TEMP");
        let previous_tmp = std::env::var_os("TMP");
        // CLI dispatch reaches this before any worker threads are created.
        std::env::set_var("TEMP", root.path());
        std::env::set_var("TMP", root.path());
        let mut viewer = Viewer::empty();
        let ctx = egui::Context::default();
        let mut stats = Stats::default();
        let mut result = exercise(&input, root.path(), &mut viewer, &ctx, &mut stats);
        if result.is_ok() {
            let large = viewer
                .entries
                .iter()
                .position(|e| e.name == input.large_entry)
                .unwrap();
            let advertised = viewer.entries[large].size;
            viewer.cache.clear();
            viewer.load_index(large, false);
            match wait_for_partial(
                &mut viewer,
                &ctx,
                &mut stats,
                root.path(),
                large,
                advertised,
            ) {
                Ok(partial) => stats.partial_bytes_before_window_close = partial,
                Err(error) => result = Err(error),
            }
        }
        // Do not exit with an outstanding detached execute()/TempDir owner.
        let cancel = viewer
            .running
            .as_ref()
            .map(|running| running.cancel.clone());
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            viewer.request_close(ctx);
            viewer.handle_close_request(ctx);
        });
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        let deferred_close = commands
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::CancelClose))
            && !commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Close));
        let close_canceled_read = cancel.is_some_and(|flag| flag.load(Ordering::Relaxed));
        let mut close_token = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !close_token {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                viewer.handle_close_request(ctx);
            });
            close_token = output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Close));
            if Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let drained =
            viewer.running.is_none() && viewer.pending.is_none() && viewer.extra.retiring.is_none();
        // egui-winit feeds Close back as a native close event on the next frame.
        // eframe checks for ANY CancelClose in that frame, so Close alone is required.
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let output = ctx.run(input, |ctx| {
            viewer.handle_close_request(ctx);
        });
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        let native_close_acknowledged = commands
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::Close))
            && !commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::CancelClose));
        drop(viewer);
        match previous_temp {
            Some(value) => std::env::set_var("TEMP", value),
            None => std::env::remove_var("TEMP"),
        }
        match previous_tmp {
            Some(value) => std::env::set_var("TMP", value),
            None => std::env::remove_var("TMP"),
        }
        let temporary_empty = std::fs::read_dir(root.path())
            .map_err(|e| e.to_string())?
            .next()
            .is_none();
        root.close()
            .map_err(|e| format!("导航测试临时目录清理失败：{e}"))?;
        if !drained || !temporary_empty || !close_token || !native_close_acknowledged {
            return Err("导航测试任务或临时目录未清空".into());
        }
        let final_image = result?;
        if !deferred_close || !close_canceled_read || stats.partial_bytes_before_window_close == 0 {
            return Err("关窗回归未取消真实的提取中途任务".into());
        }
        Ok(
            serde_json::json!({"event":"result","passed":true,"stats":stats,"final_image":final_image,
            "jobs_drained":drained,"temporary_empty":temporary_empty,
            "close_deferred":deferred_close,"close_token_after_drain":close_token,
            "close_canceled_inflight_read":close_canceled_read,
            "native_close_acknowledged_without_cancel":native_close_acknowledged,
            "scope":"One headless Viewer; real GUI navigation handlers, queue, poll, draw_image and supervised workers. No native window or filmstrip worker."}),
        )
    })();
    match result {
        Ok(report) => println!("{report}"),
        Err(error) => {
            println!(
                "{}",
                serde_json::json!({"event":"result","passed":false,"error":error})
            );
            std::process::exit(1);
        }
    }
}
