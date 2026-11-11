use crate::{
    shortcuts::{self, Action},
    worker::{self, Entry, Request},
};
use egui::{Color32, FontId, Pos2, Rect, Sense, TextStyle, TextureHandle, Vec2};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::Instant,
};

mod archive_jump;
mod archive_resume;
mod browser_ui;
mod commands;
mod compare;
mod coverflow;
mod directory;
mod edit_ui;
mod metadata_ui;
mod navigation_probe;
mod playback;
mod presentation;
mod print_ui;
mod settings_ui;
mod workspace;
use commands::Extras;
pub use navigation_probe::probe_main as navigation_probe_main;

const CPU_BUDGET: usize = 768 * 1024 * 1024;
const GPU_BUDGET: usize = 256 * 1024 * 1024;
const TILE: u32 = 1024;

struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    preview: Vec<u8>,
    preview_width: u32,
    preview_height: u32,
    note: String,
    record: Option<Arc<crate::recipes::EditRecord>>,
}
impl Pixels {
    fn bytes(&self) -> usize {
        self.rgba.len()
            + self.preview.len()
            + if self.record.is_some() {
                1024 * 1024
            } else {
                0
            }
    }
}

struct ImageCache {
    entries: HashMap<(usize, bool), Arc<Pixels>>,
    order: VecDeque<(usize, bool)>,
    bytes: usize,
    budget: usize,
}
impl Default for ImageCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            budget: CPU_BUDGET,
        }
    }
}
impl ImageCache {
    fn get(&mut self, key: (usize, bool)) -> Option<Arc<Pixels>> {
        let value = self.entries.get(&key)?.clone();
        self.order.retain(|x| *x != key);
        self.order.push_back(key);
        Some(value)
    }
    fn insert(&mut self, key: (usize, bool), value: Arc<Pixels>, pinned: Option<(usize, bool)>) {
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes();
        }
        self.order.retain(|x| *x != key);
        while self.bytes.saturating_add(value.bytes()) > self.budget && !self.order.is_empty() {
            let old = self.order.pop_front().unwrap();
            if Some(old) == pinned {
                self.order.push_back(old);
                if self.order.len() == 1 {
                    break;
                }
                continue;
            }
            if let Some(old) = self.entries.remove(&old) {
                self.bytes -= old.bytes();
            }
        }
        // A pinned current image is never displaced by speculative preloading.
        if self.bytes.saturating_add(value.bytes()) > self.budget {
            return;
        }
        self.bytes += value.bytes();
        self.entries.insert(key, value);
        self.order.push_back(key);
    }
}

#[derive(Clone)]
enum Operation {
    Rendered {
        index: usize,
    },
    Metadata {
        key: String,
    },
    CopyImage,
    Compare {
        slot: usize,
        generation: u64,
        region: Option<[u32; 4]>,
    },
    Clipboard {
        path: PathBuf,
    },
    Thumbnail {
        index: usize,
    },
    List {
        path: PathBuf,
        select: Option<String>,
        password: Option<String>,
        management: bool,
    },
    Load {
        index: usize,
        full_raw: bool,
        preload: bool,
    },
}
struct Task {
    id: u64,
    request: Request,
    operation: Operation,
}
struct Event {
    id: u64,
    request: Request,
    operation: Operation,
    result: Result<worker::Output, String>,
}
struct Running {
    id: u64,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Event>,
    preview: Option<(usize, Receiver<worker::ContinuityPreview>)>,
    preload: Option<(usize, bool)>,
    promoted: bool,
}
struct Tile {
    texture: TextureHandle,
    bytes: usize,
    used: u64,
}

pub struct Viewer {
    extra: Extras,
    archive_jump: archive_jump::State,
    archive_resume: archive_resume::State,
    #[cfg(windows)]
    native_keys: Option<crate::native_keys::NativeKeys>,
    source: Option<PathBuf>,
    entries: Vec<Entry>,
    initial_entry_position: Option<usize>,
    directory_scan: directory::Scan,
    selected: Option<usize>,
    nav_forward: bool,
    browser: bool,
    password: Option<String>,
    password_dialog: bool,
    password_focus: bool,
    password_input: String,
    retry: Option<(Request, Operation)>,
    serial: u64,
    pending: Option<Task>,
    running: Option<Running>,
    status: String,
    cache: ImageCache,
    current: Option<Arc<Pixels>>,
    current_key: Option<(usize, bool)>,
    full_raw: bool,
    overview: Option<TextureHandle>,
    tiles: HashMap<(u32, u32, u32), Tile>,
    tile_bytes: usize,
    tick: u64,
    scale: f32,
    target_scale: f32,
    pan: Vec2,
    fit: bool,
    rotation: u8,
    flip_x: bool,
    flip_y: bool,
    zoom_anchor: Option<(Pos2, Vec2)>,
    fullscreen: bool,
    last_frame: Instant,
    last_title: String,
    help: bool,
    properties: bool,
    error: Option<String>,
    closing: bool,
}

impl Viewer {
    fn empty() -> Self {
        Self {
            extra: Extras::new(),
            archive_jump: archive_jump::State::default(),
            archive_resume: archive_resume::State::default(),
            #[cfg(windows)]
            native_keys: None,
            source: None,
            entries: Vec::new(),
            initial_entry_position: None,
            directory_scan: directory::Scan::default(),
            selected: None,
            nav_forward: true,
            browser: true,
            password: None,
            password_dialog: false,
            password_focus: false,
            password_input: String::new(),
            retry: None,
            serial: 0,
            pending: None,
            running: None,
            status: "打开图片、文件夹，或 ZIP／RAR／7z 压缩包".into(),
            cache: ImageCache::default(),
            current: None,
            current_key: None,
            full_raw: false,
            overview: None,
            tiles: HashMap::new(),
            tile_bytes: 0,
            tick: 0,
            scale: 1.0,
            target_scale: 1.0,
            pan: Vec2::ZERO,
            fit: true,
            rotation: 0,
            flip_x: false,
            flip_y: false,
            zoom_anchor: None,
            fullscreen: false,
            last_frame: Instant::now(),
            last_title: String::new(),
            help: false,
            properties: false,
            error: None,
            closing: false,
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        for path in [
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/msyh.ttf",
            "C:/Windows/Fonts/simhei.ttf",
        ] {
            if let Ok(data) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("chinese".into(), Arc::new(egui::FontData::from_owned(data)));
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("chinese".into());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("chinese".into());
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.text_styles.extend([
            (TextStyle::Small, FontId::proportional(13.0)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Heading, FontId::proportional(17.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]);
        cc.egui_ctx.set_style(style);
        let mut app = Self::empty();
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            app.native_keys = cc.window_handle().ok().and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(h) => {
                    crate::native_keys::NativeKeys::attach(h.hwnd.get() as _).ok()
                }
                _ => None,
            });
        }
        app.extra.max_texture = cc
            .wgpu_render_state
            .as_ref()
            .map(|s| s.device.limits().max_texture_dimension_2d)
            .unwrap_or(4096);
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            app.extra.hwnd = cc
                .window_handle()
                .ok()
                .and_then(|h| {
                    if let RawWindowHandle::Win32(h) = h.as_raw() {
                        Some(h.hwnd.get() as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
        }
        app.load_settings();
        if let Some(path) = initial {
            app.open_path(path);
        }
        app
    }

    fn queue(&mut self, request: Request, operation: Operation) {
        if self.closing { return; }
        if matches!(operation, Operation::List { .. } | Operation::Clipboard { .. }) {
            self.directory_scan.cancel();
            self.archive_jump.open = false;
        }
        if !matches!(request, Request::MetadataRead { .. }) {
            self.stop_animation();
        }
        self.serial += 1;
        if let Some(active) = &self.running {
            active.cancel.store(true, Ordering::Relaxed);
        }
        self.pending = Some(Task {
            id: self.serial,
            request,
            operation,
        });
        self.error = None;
    }

    fn request_close(&mut self, ctx: &egui::Context) {
        if self.closing { return; }
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.status = "请等待当前文件操作完成后关闭".into();
            return;
        }
        if self.extra.edited && !confirm_discard() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        self.extra.edited = false;
        self.save_archive_resume(true);
        self.closing = true;
        self.directory_scan.cancel();
        self.serial += 1;
        self.pending = None;
        self.retry = None;
        self.password_dialog = false;
        self.password_input.clear();
        if let Some(running) = &self.running { running.cancel.store(true, Ordering::Relaxed); }
        self.stop_animation();
        self.cancel_coverflow_thumbnail();
        self.status = "正在停止读取并清理临时文件…".into();
        ctx.request_repaint_after(std::time::Duration::from_millis(15));
    }

    fn cancel_current_read(&mut self) {
        self.serial += 1;
        self.pending = None;
        if let Some(active) = &self.running {
            active.cancel.store(true, Ordering::Relaxed);
        }
        self.directory_scan.cancel();
        self.stop_animation();
        self.suspend_coverflow_thumbnails();
        self.extra.presentation.cancel_target();
        self.status = "已取消".into();
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) -> bool {
        if ctx.input(|i| i.viewport().close_requested()) { self.request_close(ctx); }
        if !self.closing { return false; }
        self.poll(ctx);
        let covers_finished = self.poll_cancelled_coverflow(ctx);
        if self.running.is_none() && self.pending.is_none() && self.extra.retiring.is_none()
            && covers_finished && self.directory_scan.is_idle()
            && self.extra.writing.is_none() && self.extra.pending_write.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        }
        true
    }

    fn open_path(&mut self, path: PathBuf) {
        if self.extra.edited
            && !rfd::MessageDialog::new()
                .set_title("尚未保存编辑")
                .set_description("继续打开其他文件会丢弃当前未保存的编辑步骤。是否继续？")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                .eq(&rfd::MessageDialogResult::Yes)
        {
            return;
        }
        self.reset_comparison();
        self.stop_animation();
        self.reset_coverflow();
        self.password_dialog = false;
        self.password_focus = false;
        self.retry = None;
        self.password_input.clear();
        let directory = path.is_dir();
        let archive = !directory && worker::is_archive(&path);
        if !directory
            && !archive
            && !path
                .file_name()
                .is_some_and(|name| crate::imaging::is_supported_image(&name.to_string_lossy()))
        {
            self.status = "该路径不存在或不是支持的图片／单卷压缩包".into();
            self.error = Some(self.status.clone());
            return;
        }
        self.directory_scan.cancel();
        if !directory && !archive {
            self.open_local_image(path);
            return;
        }
        self.status = "正在读取目录…".into();
        self.extra.workspace.preview_due = None;
        self.extra.workspace.save_pending = false;
        self.queue(
            Request::List {
                path: path.clone(),
                password: None,
            },
            Operation::List {
                path,
                select: None,
                password: None,
                management: directory,
            },
        );
    }

    fn choose_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("打开图片或压缩包")
            .pick_file()
        {
            self.open_path(path);
        }
    }
    fn choose_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("打开图片文件夹")
            .pick_folder()
        {
            self.open_path(path);
        }
    }

    fn make_load(&self, index: usize, full_raw: bool) -> Option<Request> {
        let source = self.source.as_ref()?;
        let item = self.entries.get(index)?;
        if item.directory || item.archive {
            return None;
        }
        let request = if source.is_dir() {
            Request::Load {
                path: source.join(&item.name),
                entry: None,
                password: None,
                full_raw,
            }
        } else {
            Request::Load {
                path: source.clone(),
                entry: Some(item.name.clone()),
                password: self.password.clone(),
                full_raw,
            }
        };
        Some(if self.extra.color_managed {
            request
        } else {
            Request::ColorPreview {
                source: Box::new(request),
                managed: false,
            }
        })
    }

    fn large_archived_photoshop(&self, index: usize) -> bool {
        self.entries.get(index).is_some_and(|entry| {
            entry.size > worker::MAX_INPUT_BYTES && crate::imaging::is_photoshop(&entry.name)
        }) && self.source.as_ref().is_some_and(|source| worker::is_archive(source))
    }

    fn load_index(&mut self, index: usize, full_raw: bool) {
        if self.current_key.is_some_and(|k| k.0 == index) && self.extra.edited {
            self.full_raw = full_raw;
            self.render_edits();
            return;
        }
        if self.extra.edited
            && !rfd::MessageDialog::new()
                .set_title("尚未保存编辑")
                .set_description("当前图像有未保存的编辑步骤，继续翻图会丢弃这些步骤。是否继续？")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                .eq(&rfd::MessageDialogResult::Yes)
        {
            self.extra.selection.clear();
            self.selected = self.current_key.map(|k| k.0);
            self.extra.pending_edit_id = None;
            if let Some(i) = self.selected {
                self.extra.selection.insert(i);
            }
            self.extra.coverflow.invalidate_sequence();
            return;
        }
        self.stop_animation();
        if self.extra.edited {
            self.cache = ImageCache::default();
        }
        self.extra.edits.clear();
        self.extra.redo_edits.clear();
        self.extra.edited = false;
        self.extra.workspace.preview_due = None;
        self.extra.workspace.save_pending = false;
        if self.extra.page_index != 0 {
            self.cache = ImageCache::default();
        }
        self.extra.page_index = 0;
        self.extra.page_total = 1;
        if let Some(item) = self.entries.get(index) {
            if item.directory || item.archive {
                if let Some(source) = &self.source {
                    self.open_path(source.join(&item.name));
                }
                return;
            }
        }
        let Some(request) = self.make_load(index, full_raw) else {
            return;
        };
        self.selected = Some(index);
        self.full_raw = full_raw;
        self.browser = false;
        self.extra.presentation.begin_target(index, self.entries[index].name.clone());
        self.prioritize_coverflow_preview(index);
        if let Some(image) = (!Self::is_animation_candidate(&request))
            .then(|| self.cache.get((index, full_raw)))
            .flatten()
        {
            self.serial += 1;
            self.pending = None;
            if let Some(active) = &self.running {
                active.cancel.store(true, Ordering::Relaxed);
            }
            self.set_current(index, full_raw, image);
            self.schedule_preload();
            return;
        }
        self.status = format!("正在读取 {}…", self.entries[index].name);
        self.current = None;
        self.current_key = None;
        self.overview = None;
        self.tiles.clear();
        self.tile_bytes = 0;
        if let Some(task) = &mut self.pending {
            if let Operation::Load { index: candidate, full_raw: raw, preload } = &mut task.operation {
                if *preload && (*candidate, *raw) == (index, full_raw) {
                    *preload = false;
                    return;
                }
            }
        } else if let Some(active) = &mut self.running {
            if active.id == self.serial && active.preload == Some((index, full_raw))
                && !active.cancel.load(Ordering::Relaxed) {
                active.promoted = true;
                return;
            }
        }
        self.queue(
            request,
            Operation::Load {
                index,
                full_raw,
                preload: false,
            },
        );
    }

    fn set_current(&mut self, index: usize, full_raw: bool, image: Arc<Pixels>) {
        self.extra.exposure_texture = None;
        self.selected = Some(index);
        self.current_key = Some((index, full_raw));
        self.current = Some(image);
        self.extra.presentation.ready(index);
        self.overview = None;
        self.tiles.clear();
        self.tile_bytes = 0;
        if !self.extra.zoom_lock {
            self.fit = true;
        }
        if !self.extra.pan_lock {
            self.pan = Vec2::ZERO;
        }
        self.rotation = 0;
        self.flip_x = false;
        self.flip_y = false;
        self.zoom_anchor = None;
        self.extra.selection_rect = None;
        self.status = "就绪".into();
        self.read_record();
        let record = self.current.as_ref().and_then(|p| p.record.clone());
        self.sync_workspace_source(record.as_deref());
        if let Some(id) = self.extra.pending_edit_id.take() {
            self.open_edit_dialog(id);
        }
    }

    fn schedule_preload(&mut self) {
        if self.browser
            || self.archive_jump.open
            || self.pending.is_some()
            || self.password_dialog
            || self.extra.animation.is_some()
            || self.extra.compare > 0
        {
            return;
        }
        for candidate in self.preload_neighbors(self.nav_forward)
            .into_iter()
            .flatten()
        {
            if self.cache.entries.contains_key(&(candidate, false)) {
                continue;
            }
            let Some(item) = self.entries.get(candidate) else {
                continue;
            };
            if item.directory
                || item.archive
                || crate::imaging::is_raw(&item.name)
                || matches!(
                    Path::new(&item.name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                    "gif" | "webp"
                )
                || item.size > 64 * 1024 * 1024
            {
                continue;
            }
            if self.cache.bytes > CPU_BUDGET / 2 {
                return;
            }
            if let Some(request) = self.make_load(candidate, false) {
                self.serial += 1;
                self.pending = Some(Task {
                    id: self.serial,
                    request,
                    operation: Operation::Load {
                        index: candidate,
                        full_raw: false,
                        preload: true,
                    },
                });
                return;
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_archive_resume(ctx);
        self.poll_directory_scan(ctx);
        self.poll_animation(ctx);
        self.poll_writes(ctx);
        if let Some((index, receiver)) = self.running.as_ref()
            .filter(|active| active.id == self.serial)
            .and_then(|active| active.preview.as_ref())
            .filter(|(index, _)| self.extra.presentation.target_index() == Some(*index))
        {
            if let Ok(preview) = receiver.try_recv() {
                self.extra.presentation.stage_preview(*index, preview.into());
            }
        }
        let event = match self.running.as_ref().map(|r| r.receiver.try_recv()) {
            Some(Ok(event)) => Some(event),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                if let Some(running) = self.running.take() {
                    if running.id == self.serial {
                        self.status = "后台任务意外结束，可以重新打开文件".into();
                        self.error = Some(self.status.clone());
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(mut event) = event {
            if self.running.as_ref().is_some_and(|r| r.id == event.id) {
                if self.running.take().unwrap().promoted {
                    if let Operation::Load { preload, .. } = &mut event.operation {
                        *preload = false;
                    }
                }
            }
            if event.id == self.serial {
                match event.result {
                    Ok(output) => {
                        if let Some(error) = output.reply.error.clone() {
                            if matches!(event.operation, Operation::Rendered { .. }) {
                                self.extra.workspace.save_pending = false;
                            }
                            let preload = matches!(
                                event.operation,
                                Operation::Load { preload: true, .. } | Operation::Thumbnail { .. }
                            );
                            if !preload {
                                self.status = error.clone();
                                if output.reply.password_required {
                                    self.retry = Some((event.request, event.operation));
                                    self.password_dialog = true;
                                    self.password_focus = true;
                                    self.password_input.clear();
                                } else {
                                    if let Operation::Compare { slot, .. } = event.operation {
                                        self.comparison_error(slot, error.clone());
                                    }
                                    if let Operation::Metadata { ref key } = event.operation {
                                        self.metadata_error(key.clone(), error.clone());
                                    }
                                    self.error = Some(error);
                                }
                            }
                        } else {
                            match event.operation {
                                Operation::Rendered { index } => {
                                    self.accept_render_preview(index, output)
                                }
                                Operation::Metadata { key } => {
                                    if let Some(metadata) = output.reply.metadata {
                                        self.accept_metadata(key, metadata);
                                    }
                                }
                                Operation::CopyImage => self.copy_worker_image(output),
                                Operation::Compare {
                                    slot,
                                    generation,
                                    region,
                                } => self.accept_comparison(slot, generation, region, output, ctx),
                                Operation::Clipboard { path } => {
                                    self.reset_coverflow();
                                    self.change_archive_source(None);
                                    self.source = Some(path);
                                    self.initial_entry_position = None;
                                    self.entries = vec![Entry {
                                        name: "clipboard.png".into(),
                                        size: 0,
                                        directory: false,
                                        archive: false,
                                        modified: None,
                                    }];
                                    self.extra.selection.clear();
                                    self.extra.edits.clear();
                                    self.extra.redo_edits.clear();
                                    self.extra.edited = false;
                                    self.cache = ImageCache::default();
                                    self.rebuild_visible();
                                    self.browser = false;
                                    self.set_current(
                                        0,
                                        false,
                                        Arc::new(Pixels {
                                            width: output.reply.width,
                                            height: output.reply.height,
                                            rgba: output.rgba,
                                            preview: output.preview,
                                            preview_width: output.reply.preview_width,
                                            preview_height: output.reply.preview_height,
                                            note: output.reply.note,
                                            record: None,
                                        }),
                                    );
                                }
                                Operation::Thumbnail { index } => {
                                    if self.extra.thumbnails.len() >= 384 {
                                        self.extra.thumbnails.clear();
                                    }
                                    self.extra.thumbnails.insert(
                                        index,
                                        ctx.load_texture(
                                            format!("thumb-{index}"),
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [
                                                    output.reply.width as usize,
                                                    output.reply.height as usize,
                                                ],
                                                &output.rgba,
                                            ),
                                            egui::TextureOptions::LINEAR,
                                        ),
                                    );
                                }
                                Operation::List {
                                    path,
                                    select,
                                    password,
                                    management,
                                } => {
                                    self.reset_comparison();
                                    self.reset_coverflow();
                                    self.install_source(path, password, output.reply.entries.unwrap_or_default());
                                    let selected = self.initial_entry_selection(select);
                                    self.browser = management;
                                    if let Some(index) = selected {
                                        self.load_index(index, false);
                                        if management {
                                            self.browser = true;
                                        }
                                    } else {
                                        self.status = format!(
                                            "共 {} 个条目，没有可显示的图片",
                                            self.entries.len()
                                        );
                                    }
                                }
                                Operation::Load {
                                    index,
                                    full_raw,
                                    preload,
                                } => {
                                    if !preload {
                                        self.extra.page_index = output.reply.page_index;
                                        self.extra.page_total = output.reply.page_total.max(1);
                                    }
                                    let image = Arc::new(Pixels {
                                        width: output.reply.width,
                                        height: output.reply.height,
                                        rgba: output.rgba,
                                        preview: output.preview,
                                        preview_width: output.reply.preview_width,
                                        preview_height: output.reply.preview_height,
                                        note: output.reply.note,
                                        record: output.reply.editing_record.map(Arc::new),
                                    });
                                    self.cache.insert(
                                        (index, full_raw),
                                        image.clone(),
                                        if preload { self.current_key } else { None },
                                    );
                                    if !preload {
                                        self.set_current(index, full_raw, image);
                                        self.schedule_preload();
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        if matches!(event.operation, Operation::Rendered { .. }) {
                            self.extra.workspace.save_pending = false;
                        }
                        if let Operation::Compare { slot, .. } = event.operation {
                            self.comparison_error(slot, error.clone());
                        }
                        if let Operation::Metadata { ref key } = event.operation {
                            self.metadata_error(key.clone(), error.clone());
                        }
                        if !matches!(
                            event.operation,
                            Operation::Load { preload: true, .. } | Operation::Thumbnail { .. }
                        ) && error != "已取消"
                        {
                            self.status = error.clone();
                            self.error = Some(error);
                        }
                    }
                }
            }
        }
        if self.closing { return; }
        self.schedule_edit_preview();
        if let Some(due) = self.extra.workspace.preview_due {
            ctx.request_repaint_after(
                due.saturating_duration_since(Instant::now())
                    .max(std::time::Duration::from_millis(15)),
            );
        }
        self.schedule_metadata();
        self.schedule_comparison();
        if self.running.is_none()
            && (self.extra.animation.is_none()
                || self
                    .pending
                    .as_ref()
                    .is_some_and(|t| matches!(t.operation, Operation::Metadata { .. })))
            && self.extra.retiring.is_none()
            && self.extra.writing.is_none()
            && self.extra.pending_write.is_none()
        {
            if let Some(task) = self.pending.take() {
                if Self::is_animation_candidate(&task.request)
                    && !self.browser
                    && matches!(task.operation, Operation::Load { preload: false, .. })
                {
                    self.start_animation(task, ctx);
                    return;
                }
                let (sender, receiver) = mpsc::sync_channel(1);
                let (preview_tx, preview_rx) = mpsc::sync_channel(1);
                let (preview_index, preload) = match &task.operation {
                    Operation::Load { index, full_raw, preload } =>
                        (Some(*index), preload.then_some((*index, *full_raw))),
                    _ => (None, None),
                };
                let cancel = Arc::new(AtomicBool::new(false));
                let thread_cancel = cancel.clone();
                let context = ctx.clone();
                let id = task.id;
                std::thread::spawn(move || {
                    let request = task.request.clone();
                    let result = if preview_index.is_some() {
                        worker::execute_with_preview(task.request, thread_cancel, |preview| {
                            let _ = preview_tx.try_send(preview);
                            context.request_repaint();
                        })
                    } else {
                        worker::execute(task.request, thread_cancel)
                    };
                    let _ = sender.send(Event {
                        id,
                        request,
                        operation: task.operation,
                        result,
                    });
                    context.request_repaint();
                });
                self.running = Some(Running {
                    id,
                    cancel,
                    receiver,
                    preview: preview_index.map(|index| (index, preview_rx)),
                    preload,
                    promoted: false,
                });
            }
        }
        if self.running.is_some() || self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    fn navigate(&mut self, forward: bool, end: bool) {
        if let Some(index) = self.navigation_target(forward, end) {
            self.nav_forward = forward;
            self.load_index(index, false);
        }
    }

    fn reset_fit(&mut self) {
        self.fit = true;
        self.pan = Vec2::ZERO;
        self.zoom_anchor = None;
    }
    fn zoom_by(&mut self, factor: f32) {
        self.fit = false;
        self.target_scale = (self.target_scale * factor).clamp(0.0001, 64.0);
        self.zoom_anchor = None;
    }

    fn perform(&mut self, action: Action, ctx: &egui::Context) {
        if self.closing { return; }
        if self.comparison_action(action, ctx) {
            return;
        }
        match action {
            Action::LegacyCommand(command) => self.legacy_command(command, ctx),
            Action::DeletePermanently => self.delete_selected(true),
            Action::ToggleAnimation => self.toggle_animation(),
            Action::Open => self.choose_file(),
            Action::OpenFolder => self.choose_folder(),
            Action::Next => self.navigate(true, false),
            Action::Previous => self.navigate(false, false),
            Action::First => self.navigate(false, true),
            Action::Last => self.navigate(true, true),
            Action::ZoomIn => self.zoom_by(1.25),
            Action::ZoomOut => self.zoom_by(0.8),
            Action::Fit => self.reset_fit(),
            Action::ActualSize => {
                self.fit = false;
                self.target_scale = 1.0 / ctx.pixels_per_point();
                self.pan = Vec2::ZERO;
                self.zoom_anchor = None;
            }
            Action::Fullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Action::RotateLeft => {
                self.rotation = (self.rotation + 3) % 4;
                self.reset_fit();
            }
            Action::RotateRight => {
                self.rotation = (self.rotation + 1) % 4;
                self.reset_fit();
            }
            Action::FlipHorizontal => self.flip_x = !self.flip_x,
            Action::FlipVertical => self.flip_y = !self.flip_y,
            Action::ToggleBrowser => {
                if self.fullscreen {
                    self.fullscreen = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                } else {
                    if self.browser {
                        self.open_management_selection(ctx);
                    } else {
                        self.browser = true;
                        self.stop_animation();
                    }
                    if self.browser && self.extra.selection.is_empty() {
                        if let Some(i) = self.selected {
                            self.extra.selection.insert(i);
                        }
                    }
                }
            }
            Action::Refresh => {
                if self.extra.edited
                    && !rfd::MessageDialog::new()
                        .set_title("重新读取原图")
                        .set_description("刷新会丢弃当前编辑步骤并重新读取原图。是否继续？")
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        .eq(&rfd::MessageDialogResult::Yes)
                {
                    return;
                }
                if let Some(path) = self.source.clone() {
                    let select = self
                        .selected
                        .and_then(|i| self.entries.get(i))
                        .map(|e| e.name.clone());
                    let password = self.password.clone();
                    self.queue(
                        Request::List {
                            path: path.clone(),
                            password: password.clone(),
                        },
                        Operation::List {
                            path,
                            select,
                            password,
                            management: self.browser,
                        },
                    );
                }
            }
            Action::Close => {
                if self.extra.edited && !confirm_discard() {
                    return;
                }
                self.extra.edited = false;
                self.extra.edits.clear();
                self.extra.redo_edits.clear();
                self.stop_animation();
                self.directory_scan.cancel();
                self.serial += 1;
                self.pending = None;
                if let Some(r) = &self.running {
                    r.cancel.store(true, Ordering::Relaxed);
                }
                self.browser = true;
                self.extra.presentation.clear();
                self.current = None;
                self.current_key = None;
                self.selected = None;
                self.overview = None;
                self.tiles.clear();
                self.tile_bytes = 0;
            }
            Action::Exit => {
                self.request_close(ctx);
                self.handle_close_request(ctx);
            }
            Action::Properties => {
                if self.properties {
                    self.properties = false;
                } else {
                    self.open_source_metadata(None);
                }
            }
            Action::CopyPath => {
                if let Some(path) = self.current_path() {
                    ctx.copy_text(path);
                }
            }
            Action::CopyFile => {
                self.copy_file_selection();
            }
            Action::CopyImage => {
                if let Some(image) = &self.current {
                    let transformed = if self.rotation != 0 || self.flip_x || self.flip_y {
                        match transformed_rgba(image, self.rotation, self.flip_x, self.flip_y) {
                            Ok(p) => Some(p),
                            Err(e) => {
                                self.error = Some(e);
                                return;
                            }
                        }
                    } else {
                        None
                    };
                    let (width, height, bytes) = if let Some((w, h, p)) = transformed {
                        (w, h, std::borrow::Cow::Owned(p))
                    } else {
                        (
                            image.width,
                            image.height,
                            std::borrow::Cow::Borrowed(image.rgba.as_slice()),
                        )
                    };
                    match arboard::Clipboard::new().and_then(|mut c| {
                        c.set_image(arboard::ImageData {
                            width: width as usize,
                            height: height as usize,
                            bytes,
                        })
                    }) {
                        Ok(()) => self.status = "图像已复制".into(),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            Action::Delete => {
                self.legacy_command(2031, ctx);
            }
        }
    }

    fn current_path(&self) -> Option<String> {
        let source = self.source.as_ref()?;
        let entry = self.entries.get(self.selected?)?;
        Some(if source.is_dir() {
            source.join(&entry.name).display().to_string()
        } else {
            format!("{} :: {}", source.display(), entry.name)
        })
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        if !self.extra.toolbar {
            return;
        }
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("文件", |ui| {
                    for (label, action) in [
                        ("打开图片或压缩包…", Action::Open),
                        ("打开文件夹…", Action::OpenFolder),
                        ("刷新", Action::Refresh),
                        ("属性", Action::Properties),
                        ("关闭当前图像", Action::Close),
                        ("退出", Action::Exit),
                    ] {
                        if ui.button(label).clicked() {
                            self.perform(action, ctx);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("编辑", |ui| {
                    for (label, action) in [
                        ("复制文件", Action::CopyFile),
                        ("复制图像", Action::CopyImage),
                        ("复制路径", Action::CopyPath),
                        ("移到回收站", Action::Delete),
                    ] {
                        if ui.button(label).clicked() {
                            self.perform(action, ctx);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("视图", |ui| {
                    for (label, action) in [
                        ("文件列表", Action::ToggleBrowser),
                        ("适应窗口", Action::Fit),
                        ("原始尺寸", Action::ActualSize),
                        ("全屏", Action::Fullscreen),
                        ("向左旋转", Action::RotateLeft),
                        ("向右旋转", Action::RotateRight),
                        ("水平翻转", Action::FlipHorizontal),
                        ("垂直翻转", Action::FlipVertical),
                    ] {
                        if ui.button(label).clicked() {
                            self.perform(action, ctx);
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("快捷键与版本说明").clicked() {
                    self.help = true;
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("打开…").clicked() {
                    self.choose_file();
                }
                if ui.button("文件夹…").clicked() {
                    self.choose_folder();
                }
                ui.separator();
                for (label, action) in [
                    ("上一张", Action::Previous),
                    ("下一张", Action::Next),
                    ("−", Action::ZoomOut),
                    ("＋", Action::ZoomIn),
                    ("适应", Action::Fit),
                    ("1:1", Action::ActualSize),
                    ("全屏", Action::Fullscreen),
                ] {
                    if ui.button(label).clicked() {
                        self.perform(action, ctx);
                    }
                }
                if self.is_archive_source() && !self.browser
                    && self.extra.compare == 0 && self.extra.edit_dialog.is_none()
                {
                    let available = !self.password_dialog && !self.browse_entries().1.is_empty();
                    if ui.add_enabled(available, egui::Button::new("跳页…")).clicked() {
                        self.open_archive_jump();
                    }
                }
                let is_raw = self
                    .selected
                    .and_then(|i| self.entries.get(i))
                    .is_some_and(|e| crate::imaging::is_raw(&e.name));
                if is_raw {
                    let mut full = self.full_raw;
                    if ui.checkbox(&mut full, "完整 RAW 解码").changed() {
                        if let Some(i) = self.selected {
                            self.load_index(i, full);
                        }
                    }
                }
                if (self.running.is_some() || !self.directory_scan.is_idle())
                    && self.extra.pending_write.is_none()
                    && self.extra.writing.is_none()
                {
                    ui.spinner();
                    if ui.small_button("取消").clicked() {
                        self.cancel_current_read();
                    }
                }
                if self.extra.animated {
                    if ui
                        .button(if self.extra.animation_ended {
                            "重播动画 (G)"
                        } else if self.extra.animation_paused {
                            "播放动画 (G)"
                        } else {
                            "暂停动画 (G)"
                        })
                        .clicked()
                    {
                        self.toggle_animation();
                    }
                }
                if self.extra.edited {
                    ui.colored_label(Color32::YELLOW, "有未保存的编辑");
                }
            });
            if let Some(path) = &self.source {
                let path = path.to_string_lossy();
                let label = egui::RichText::new(path.as_ref()).small().weak();
                ui.add(egui::Label::new(label).truncate())
                    .on_hover_text(path.as_ref());
            }
        });
    }

    fn ensure_overview(&mut self, ctx: &egui::Context) {
        if self.overview.is_none() {
            if let Some(image) = &self.current {
                self.overview = Some(ctx.load_texture(
                    "overview",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [image.preview_width as usize, image.preview_height as usize],
                        &image.preview,
                    ),
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
    }

    fn draw_image(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (viewport, response) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.extra.viewport = viewport;
        let painter = ui.painter_at(viewport);
        painter.rect_filled(viewport, 0.0, Color32::from_rgb(23, 24, 26));
        if let Some(texture) = &self.extra.background_tile {
            let period = texture.size_vec2() / ctx.pixels_per_point();
            painter.image(
                texture.id(),
                viewport,
                Rect::from_min_max(
                    Pos2::ZERO,
                    egui::pos2(viewport.width() / period.x, viewport.height() / period.y),
                ),
                Color32::WHITE,
            );
        }
        let Some(image) = self.current.clone() else {
            if self.extra.presentation.draw(&painter, viewport, ctx) {
                return;
            }
            painter.text(
                viewport.center() - egui::vec2(0.0, 18.0),
                egui::Align2::CENTER_CENTER,
                "RustView",
                FontId::proportional(24.0),
                Color32::LIGHT_GRAY,
            );
            painter.text(
                viewport.center() + egui::vec2(0.0, 15.0),
                egui::Align2::CENTER_CENTER,
                "打开或拖入图片、文件夹或 ZIP／RAR／7z",
                TextStyle::Body.resolve(ui.style()),
                Color32::GRAY,
            );
            return;
        };
        let dimensions = if self.rotation % 2 == 0 {
            egui::vec2(image.width as f32, image.height as f32)
        } else {
            egui::vec2(image.height as f32, image.width as f32)
        };
        if self.fit {
            let fit = (viewport.width() / dimensions.x)
                .min(viewport.height() / dimensions.y)
                .min(1.0 / ctx.pixels_per_point());
            self.scale = fit;
            self.target_scale = fit;
        }
        if response.hovered() && !self.password_dialog {
            let scroll = ctx.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(mouse) = response.hover_pos() {
                    self.fit = false;
                    self.zoom_anchor =
                        Some((mouse, (mouse - viewport.center() - self.pan) / self.scale));
                    self.target_scale =
                        (self.target_scale * (scroll * 0.0025).exp()).clamp(0.0001, 64.0);
                }
            }
        }
        if response.dragged() && self.extra.tool == 2371 {
            self.pan += response.drag_delta();
            self.fit = false;
            self.zoom_anchor = None;
        }
        if response.double_clicked() {
            self.reset_fit();
        }
        if self.extra.tool == 2372 {
            if response.drag_started() {
                self.extra.selection_origin = response.interact_pointer_pos();
            }
            if response.dragged() {
                if let (Some(a), Some(b)) =
                    (self.extra.selection_origin, response.interact_pointer_pos())
                {
                    self.extra.selection_rect = Some(Rect::from_two_pos(a, b).intersect(viewport));
                }
            }
        }
        if self.extra.tool == 2373 && response.clicked() {
            self.zoom_by(if ctx.input(|i| i.modifiers.shift) {
                0.5
            } else {
                2.0
            });
        }
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.1);
        self.last_frame = Instant::now();
        if (self.scale / self.target_scale - 1.0).abs() > 0.0001 {
            self.scale = (self.scale.ln()
                + (self.target_scale.ln() - self.scale.ln()) * (1.0 - (-dt * 22.0).exp()))
            .exp();
            if let Some((mouse, anchor)) = self.zoom_anchor {
                self.pan = mouse - viewport.center() - anchor * self.scale;
            }
            ctx.request_repaint();
        } else {
            self.scale = self.target_scale;
        }
        let center = viewport.center() + self.pan;
        self.ensure_overview(ctx);
        let detail = (image.width as f32 * self.scale * ctx.pixels_per_point()
            > image.preview_width as f32 * 1.05
            || image.height as f32 * self.scale * ctx.pixels_per_point()
                > image.preview_height as f32 * 1.05)
            && !(self.extra.animated && !self.extra.animation_paused);
        let full_uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
        paint_transparency(&painter, Rect::from_center_size(center, dimensions * self.scale));
        if let Some(texture) = &self.extra.animation_texture {
            paint_quad(
                &painter,
                texture.id(),
                Rect::from_min_max(
                    Pos2::ZERO,
                    egui::pos2(image.width as f32, image.height as f32),
                ),
                full_uv,
                &image,
                center,
                self.scale,
                self.rotation,
                self.flip_x,
                self.flip_y,
            );
        } else if !detail {
            if let Some(texture) = &self.overview {
                paint_quad(
                    &painter,
                    texture.id(),
                    Rect::from_min_max(
                        Pos2::ZERO,
                        egui::pos2(image.width as f32, image.height as f32),
                    ),
                    full_uv,
                    &image,
                    center,
                    self.scale,
                    self.rotation,
                    self.flip_x,
                    self.flip_y,
                );
            }
        } else {
            let bounds = visible_image_bounds(
                viewport,
                center,
                self.scale,
                self.rotation,
                self.flip_x,
                self.flip_y,
                image.width,
                image.height,
            );
            let level = tile_level(
                self.scale * ctx.pixels_per_point(),
                bounds,
                image.width,
                image.height,
            );
            let step = 1u32 << level;
            let span = TILE * step;
            let x0 =
                (bounds.min.x.max(0.0) as u32 / span).min(image.width.saturating_sub(1) / span);
            let y0 =
                (bounds.min.y.max(0.0) as u32 / span).min(image.height.saturating_sub(1) / span);
            let x1 = ((bounds.max.x.max(0.0) as u32 / span) + 1).min(image.width.div_ceil(span));
            let y1 = ((bounds.max.y.max(0.0) as u32 / span) + 1).min(image.height.div_ceil(span));
            let mut uploads = 0;
            for ty in y0..y1 {
                for tx in x0..x1 {
                    let key = (level, tx, ty);
                    self.tick += 1;
                    let x = tx * span;
                    let y = ty * span;
                    let source_w = span.min(image.width - x);
                    let source_h = span.min(image.height - y);
                    let rect = Rect::from_min_max(
                        egui::pos2(x as f32, y as f32),
                        egui::pos2((x + source_w) as f32, (y + source_h) as f32),
                    );
                    if !self.tiles.contains_key(&key) && uploads < 2 {
                        let w = source_w.div_ceil(step);
                        let h = source_h.div_ceil(step);
                        let bytes = w as usize * h as usize * 4;
                        while self.tile_bytes + bytes > GPU_BUDGET {
                            let oldest = self
                                .tiles
                                .iter()
                                .min_by_key(|(_, t)| t.used)
                                .map(|(k, _)| *k);
                            if let Some(old) = oldest {
                                if let Some(tile) = self.tiles.remove(&old) {
                                    self.tile_bytes -= tile.bytes;
                                }
                            } else {
                                break;
                            }
                        }
                        let mut pixels = Vec::with_capacity(bytes);
                        for row in 0..h {
                            let source_y = (y + row * step).min(image.height - 1);
                            for col in 0..w {
                                let source_x = (x + col * step).min(image.width - 1);
                                let start = (source_y as usize * image.width as usize
                                    + source_x as usize)
                                    * 4;
                                pixels.extend_from_slice(&image.rgba[start..start + 4]);
                            }
                        }
                        let texture = ctx.load_texture(
                            format!("tile-{level}-{tx}-{ty}"),
                            egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &pixels,
                            ),
                            egui::TextureOptions::LINEAR,
                        );
                        self.tiles.insert(
                            key,
                            Tile {
                                texture,
                                bytes,
                                used: self.tick,
                            },
                        );
                        self.tile_bytes += bytes;
                        uploads += 1;
                    }
                    if let Some(tile) = self.tiles.get_mut(&key) {
                        tile.used = self.tick;
                        paint_quad(
                            &painter,
                            tile.texture.id(),
                            rect,
                            full_uv,
                            &image,
                            center,
                            self.scale,
                            self.rotation,
                            self.flip_x,
                            self.flip_y,
                        );
                    } else {
                        if let Some(texture) = &self.overview {
                            let uv = Rect::from_min_max(
                                egui::pos2(
                                    rect.min.x / image.width as f32,
                                    rect.min.y / image.height as f32,
                                ),
                                egui::pos2(
                                    rect.max.x / image.width as f32,
                                    rect.max.y / image.height as f32,
                                ),
                            );
                            paint_quad(
                                &painter,
                                texture.id(),
                                rect,
                                uv,
                                &image,
                                center,
                                self.scale,
                                self.rotation,
                                self.flip_x,
                                self.flip_y,
                            );
                        }
                        ctx.request_repaint();
                    }
                }
            }
        }
        if self.extra.exposure_warning {
            if self.extra.exposure_texture.is_none() {
                let mut mask = Vec::with_capacity(image.preview.len());
                for p in image.preview.chunks_exact(4) {
                    let color = if p[3] == 0 {
                        [0, 0, 0, 0]
                    } else if p[..3].iter().any(|c| *c >= 250) {
                        [255, 30, 10, 160]
                    } else if p[..3].iter().all(|c| *c <= 5) {
                        [20, 80, 255, 160]
                    } else {
                        [0, 0, 0, 0]
                    };
                    mask.extend_from_slice(&color);
                }
                self.extra.exposure_texture = Some(ctx.load_texture(
                    "exposure-warning",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [image.preview_width as usize, image.preview_height as usize],
                        &mask,
                    ),
                    egui::TextureOptions::NEAREST,
                ));
            }
            if let Some(texture) = &self.extra.exposure_texture {
                paint_quad(
                    &painter,
                    texture.id(),
                    Rect::from_min_max(
                        Pos2::ZERO,
                        egui::pos2(image.width as f32, image.height as f32),
                    ),
                    full_uv,
                    &image,
                    center,
                    self.scale,
                    self.rotation,
                    self.flip_x,
                    self.flip_y,
                );
            }
        }
        if let Some(r) = self.extra.selection_rect {
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0_f32, Color32::LIGHT_BLUE),
                egui::StrokeKind::Inside,
            );
        }
        if self.extra.magnifier {
            if let Some(mouse) = response.hover_pos() {
                if let Some(t) = &self.overview {
                    let pixel = (mouse - center) / self.scale
                        + egui::vec2(image.width as f32, image.height as f32) * 0.5;
                    let half = egui::vec2(
                        100.0 / self.extra.magnification / image.width as f32,
                        100.0 / self.extra.magnification / image.height as f32,
                    );
                    let uv = Pos2::new(pixel.x / image.width as f32, pixel.y / image.height as f32);
                    let r = Rect::from_min_size(
                        mouse + egui::vec2(24.0, 24.0),
                        egui::vec2(200.0, 200.0),
                    );
                    paint_transparency(&painter, r);
                    painter.image(
                        t.id(),
                        r,
                        Rect::from_center_size(uv, half * 2.0),
                        Color32::WHITE,
                    );
                    painter.rect_stroke(
                        r,
                        0.0,
                        egui::Stroke::new(1.0_f32, Color32::WHITE),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        if let Some((index, _)) = self.current_key {
            let placement = presentation::Placement {
                scale: self.scale, pan: self.pan, rotation: self.rotation,
                flip_x: self.flip_x, flip_y: self.flip_y, fit: self.fit,
            };
            let name = self.entries.get(index).map(|entry| entry.name.as_str()).unwrap_or("");
            if let Some((preview, texture)) = self.extra.presentation.capture_displayed(
                ctx, index, &image, Arc::as_ptr(&image) as usize, placement, name,
            ) {
                self.remember_coverflow_preview(index, preview, texture);
                self.archive_image_drawn(index, ctx);
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.draw_archive_jump(ctx);
        if self.password_dialog {
            let mut submit = false;
            let mut cancel = false;
            egui::Modal::new(egui::Id::new("archive-password")).show(ctx, |ui| {
                ui.heading("压缩包密码");
                ui.label(&self.status);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.password_input)
                        .password(true)
                        .desired_width(300.0),
                );
                if self.password_focus {
                    response.request_focus();
                    self.password_focus = false;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("打开").clicked() {
                        submit = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
                ui.small("密码只用于当前会话，不写入设置或日志。");
            });
            if cancel {
                self.password_dialog = false;
                self.retry = None;
                self.password_input.clear();
                self.status = "已取消密码输入".into();
            }
            if submit {
                if let Some((mut request, mut operation)) = self.retry.take() {
                    let password = Some(std::mem::take(&mut self.password_input));
                    request.set_password(password.clone());
                    match &mut operation {
                        Operation::List { password: p, .. } => *p = password.clone(),
                        _ => self.password = password,
                    }
                    self.reset_coverflow();
                    self.password_dialog = false;
                    self.status = "正在验证密码…".into();
                    self.queue(request, operation);
                }
            }
        }
        if self.help {
            egui::Window::new("快捷键与版本说明").open(&mut self.help).default_width(560.0).show(ctx,|ui|{
            ui.label(concat!("RustView ", env!("CARGO_PKG_VERSION"), " · Windows x64 · Rust / egui / wgpu"));
            ui.separator();
            ui.label("查看模式：←/→ 上一张/下一张；↑/↓ 平滑放大/缩小。Space / PageDown 下一张；Backspace / PageUp 上一张；Home / End 首末张；F 全屏；Ctrl+Alt+←/→ 旋转。");
            ui.label("数字键盘 + / − 缩放；* 适应窗口；/ 原始尺寸。滚轮平滑缩放，鼠标拖动平移，双击恢复适应。底部封面栏可点击翻图，并显示当前批次序号。");
            ui.label("ZIP／RAR／7z 会记住上次显示的图片。工具栏「跳页…」可按文件名、包内路径或当前序号查找。");
            ui.label("查看模式 Enter / Esc / Ctrl+W 返回列表；列表 Enter进入查看、Ctrl+W退出；Ctrl/Shift+单击多选。GIF/WebP按原帧时长播放，G暂停／恢复／重播。");
            ui.separator();ui.label("ZIP／7z包内修改先备份原包；RAR只读，不支持分卷。编辑支持高位副本、完整动画、无损参数记录；源图EXIF／IPTC／XMP在属性窗口中显式保存，RAW写XMP伴随文件。");
            ui.label("幻灯片、日历、刻录、在线和音视频均已移除。默认键位见docs/KEYBOARD.md，功能状态见docs/retained-command-audit.csv；未确认的原版隐藏命令不激活。");
            ui.label("CPU图像缓存 768 MiB；细节纹理 256 MiB；单个工作进程提交内存 2 GiB；单文件输入及RGBA输出分别限512 MiB。超限文件会报告错误。");
        });
        }
        self.draw_source_metadata(ctx);
        self.draw_print_dialog(ctx);
        self.draw_settings(ctx);
        if let Some(error) = self.error.clone() {
            let mut open = true;
            let mut original = false;
            egui::Window::new("读取提示")
                .open(&mut open)
                .default_width(500.0)
                .show(ctx, |ui| {
                    ui.label(error);
                    if self
                        .selected
                        .and_then(|i| self.make_view_load(i, self.full_raw))
                        .is_some()
                        && ui.button("查看原图（暂不应用编辑记录）").clicked()
                    {
                        original = true;
                    }
                    if ui.button("知道了").clicked() {
                        self.error = None;
                    }
                });
            if !open {
                self.error = None;
            }
            if original && (!self.extra.edited || confirm_discard()) {
                if let Some((index, source)) = self
                    .selected
                    .and_then(|i| self.make_view_load(i, self.full_raw).map(|r| (i, r)))
                {
                    self.extra.edits.clear();
                    self.extra.redo_edits.clear();
                    self.extra.edited = false;
                    self.extra.workspace = workspace::State::default();
                    self.cache = ImageCache::default();
                    self.error = None;
                    self.reset_comparison();
                    self.browser=false;
                    self.queue(
                        Request::Unedited {
                            source: Box::new(source),
                        },
                        Operation::Load {
                            index,
                            full_raw: self.full_raw,
                            preload: false,
                        },
                    );
                }
            }
        }
    }
}

impl eframe::App for Viewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.handle_close_request(ctx) { return; }
        self.poll(ctx);
        let dropped = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.open_path(path);
        }
        // Focused list/buttons keep application shortcuts; text fields consume typing.
        #[cfg(windows)]
        let native_events = self
            .native_keys
            .as_ref()
            .map(|n| n.drain())
            .unwrap_or_default();
        #[cfg(windows)]
        if self.extra.settings_ui.open {
            let _ = self.custom_native_action(&native_events, 2);
        }
        if shortcuts_enabled(
            ctx,
            self.password_dialog
                || self.archive_jump.open
                || self.extra.dialog.is_some()
                || self.properties
                || self.extra.settings_ui.open
                || self.extra.print_ui.open
                || self.help,
        ) && (!text_field_focused(ctx)
            || self.extra.edit_dialog.is_some()
                && ctx.input(|i| {
                    i.modifiers.ctrl
                        && !i.modifiers.alt
                        && !i.modifiers.shift
                        && i.key_pressed(egui::Key::S)
                }))
        {
            let single_view = !self.browser
                && self.extra.compare == 0
                && self.extra.edit_dialog.is_none();
            #[cfg(windows)]
            let action = if self.native_keys.is_some() && !native_events.is_empty() {
                let table = if self.extra.edit_dialog.is_some() {
                    22118
                } else if self.extra.compare > 0 {
                    553
                } else if self.browser {
                    2
                } else {
                    552
                };
                match self.custom_native_action(&native_events, table) {
                    Some(action) => action,
                    None => {
                        shortcuts::native_action_for_table_with_view_arrows(&native_events, table, single_view).or_else(|| {
                            let bare_view_arrow = native_events.iter().any(|event| matches!(event,
                                crate::native_keys::NativeKeyEvent::Key {
                                    virtual_key: 0x25..=0x28, pressed: true, modifiers, ..
                                } if modifiers.is_none()));
                            if (table == 553 || table == 22118) && !bare_view_arrow {
                                self.custom_native_action(&native_events, 552)
                                    .unwrap_or_else(|| {
                                        shortcuts::native_action_for_table_with_view_arrows(&native_events, 552, false)
                                    })
                            } else {
                                None
                            }
                        })
                    }
                }
            } else {
                ctx.input(|i| shortcuts::action_with_view_arrows(i, self.browser, single_view))
            };
            #[cfg(not(windows))]
            let action = ctx.input(|i| shortcuts::action_with_view_arrows(i, self.browser, single_view));
            let management_navigation = self.browser && ctx.input(|i| i.events.iter().any(|e| matches!(e,
                egui::Event::Key { key: egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Home | egui::Key::End | egui::Key::PageUp | egui::Key::PageDown, pressed: true, modifiers, .. } if !modifiers.ctrl && !modifiers.alt)));
            let processing_button_key = self.extra.edit_dialog.is_some()
                && ctx.wants_keyboard_input()
                && ctx.input(|i| {
                    i.modifiers.is_none()
                        && (i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter))
                });
            if let Some(action) = action.filter(|a| {
                !processing_button_key
                    && !(management_navigation
                        && matches!(
                            a,
                            Action::Next | Action::Previous | Action::First | Action::Last
                        ))
            }) {
                if single_view {
                    ctx.input_mut(consume_view_arrows);
                }
                self.perform(action, ctx);
            }
        }
        self.draw_toolbar(ctx);
        self.draw_extra_panels(ctx);
        self.draw_browser_panels(ctx);
        self.edit_dialogs(ctx);
        if self.extra.statusbar {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(format!(
                        "CPU {:.0}/768 MiB · GPU细节 {:.0}/256 MiB",
                        self.cache.bytes as f64 / 1048576.0,
                        self.tile_bytes as f64 / 1048576.0
                    ));
                    if let Some(image) = &self.current {
                        ui.separator();
                        let (width, height) = self
                            .extra
                            .comparison
                            .active_dimensions()
                            .unwrap_or((image.width, image.height));
                        ui.small(format!(
                            "{} × {}  · {:.0}%",
                            width,
                            height,
                            self.scale * ctx.pixels_per_point() * 100.0
                        ));
                    }
                    ui.separator();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), ui.text_style_height(&TextStyle::Small)),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let label = egui::RichText::new(&self.status).small();
                            ui.add(egui::Label::new(label).truncate())
                                .on_hover_text(&self.status);
                        },
                    );
                });
            });
        }
        self.poll_coverflow(ctx);
        self.draw_coverflow(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                if self.extra.compare > 0 {
                    self.draw_comparison(ui, ctx)
                } else if self.browser && self.extra.layout != 2014 {
                    self.draw_management(ui, ctx)
                } else {
                    self.draw_image(ui, ctx)
                }
            });
        self.dialogs(ctx);
        self.command_dialogs(ctx);
        self.draw_inspectors(ctx);
        let title = self
            .selected
            .and_then(|i| self.entries.get(i))
            .map(|e| format!("{} — RustView", e.name))
            .unwrap_or_else(|| "RustView 图片浏览器".into());
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        self.save_archive_resume(true);
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn shortcuts_enabled(ctx: &egui::Context, password_dialog: bool) -> bool {
    !password_dialog && !ctx.memory(|m| m.any_popup_open())
}

fn consume_view_arrows(input: &mut egui::InputState) {
    input.events.retain(|event| !matches!(event,
        egui::Event::Key {
            key: egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown,
            pressed: true, modifiers, ..
        } if modifiers.is_none()));
}

fn confirm_discard() -> bool {
    rfd::MessageDialog::new()
        .set_title("尚未保存编辑")
        .set_description("当前编辑尚未保存为副本，是否丢弃这些步骤并继续？")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}
fn text_field_focused(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused())
        .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some())
}

fn rotate(v: Vec2, r: u8) -> Vec2 {
    match r % 4 {
        1 => egui::vec2(-v.y, v.x),
        2 => -v,
        3 => egui::vec2(v.y, -v.x),
        _ => v,
    }
}

fn transformed_rgba(
    image: &Pixels,
    rotation: u8,
    flip_x: bool,
    flip_y: bool,
) -> Result<(u32, u32, Vec<u8>), String> {
    let (w, h) = (image.width, image.height);
    let expected = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n <= worker::MAX_IMAGE_BYTES && *n == image.rgba.len())
        .ok_or("复制图像尺寸无效或超过预算")?;
    let (out_w, out_h) = if rotation % 2 == 0 { (w, h) } else { (h, w) };
    let mut output = Vec::new();
    output
        .try_reserve_exact(expected)
        .map_err(|_| "无法分配旋转复制缓冲")?;
    output.resize(expected, 0);
    for y in 0..h {
        for x in 0..w {
            let a = if flip_x { w - 1 - x } else { x };
            let b = if flip_y { h - 1 - y } else { y };
            let (a, b) = match rotation % 4 {
                0 => (a, b),
                1 => (h - 1 - b, a),
                2 => (w - 1 - a, h - 1 - b),
                _ => (b, w - 1 - a),
            };
            let source = (y as usize * w as usize + x as usize) * 4;
            let target = (b as usize * out_w as usize + a as usize) * 4;
            output[target..target + 4].copy_from_slice(&image.rgba[source..source + 4]);
        }
    }
    Ok((out_w, out_h, output))
}
fn paint_transparency(painter: &egui::Painter, rect: Rect) {
    let visible = rect.intersect(painter.clip_rect());
    if !visible.is_positive() {
        return;
    }
    let ctx = painter.ctx();
    let id = egui::Id::new("transparency-checkerboard");
    let cached = ctx.data(|data| data.get_temp::<TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let light = Color32::from_gray(224);
        let dark = Color32::from_gray(176);
        let texture = ctx.load_texture(
            "transparency-checkerboard",
            egui::ColorImage { size: [2, 2], pixels: vec![light, dark, dark, light] },
            egui::TextureOptions::NEAREST_REPEAT,
        );
        ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    // Window coordinates keep the eight-point cells stationary as the image moves or zooms.
    let uv = Rect::from_min_max(
        (visible.min.to_vec2() / 16.0).to_pos2(),
        (visible.max.to_vec2() / 16.0).to_pos2(),
    );
    painter.image(texture.id(), visible, uv, Color32::WHITE);
}

fn paint_quad(
    painter: &egui::Painter,
    texture: egui::TextureId,
    rect: Rect,
    uv_rect: Rect,
    image: &Pixels,
    center: Pos2,
    scale: f32,
    rotation: u8,
    flip_x: bool,
    flip_y: bool,
) {
    let mut mesh = egui::Mesh::with_texture(texture);
    let half = egui::vec2(image.width as f32, image.height as f32) * 0.5;
    for (p, uv) in [
        (rect.left_top(), uv_rect.left_top()),
        (rect.right_top(), uv_rect.right_top()),
        (rect.left_bottom(), uv_rect.left_bottom()),
        (rect.right_bottom(), uv_rect.right_bottom()),
    ] {
        let mut v = p.to_vec2() - half;
        if flip_x {
            v.x = -v.x;
        }
        if flip_y {
            v.y = -v.y;
        }
        let pos = center + rotate(v, rotation) * scale;
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
    painter.add(egui::Shape::mesh(mesh));
}
fn visible_image_bounds(
    view: Rect,
    center: Pos2,
    scale: f32,
    rotation: u8,
    flip_x: bool,
    flip_y: bool,
    w: u32,
    h: u32,
) -> Rect {
    let mut bounds = Rect::NOTHING;
    for p in [
        view.left_top(),
        view.right_top(),
        view.left_bottom(),
        view.right_bottom(),
    ] {
        let mut v = rotate((p - center) / scale, (4 - rotation) % 4);
        if flip_x {
            v.x = -v.x;
        }
        if flip_y {
            v.y = -v.y;
        }
        bounds.extend_with(Pos2::ZERO + v + egui::vec2(w as f32, h as f32) * 0.5);
    }
    bounds
}

fn tile_level(physical_scale: f32, bounds: Rect, w: u32, h: u32) -> u32 {
    let mut level = (1.0 / physical_scale.max(0.000001))
        .log2()
        .floor()
        .max(0.0)
        .min(16.0) as u32;
    loop {
        let span = TILE * (1u32 << level);
        let columns = ((bounds.max.x.max(0.0).min(w as f32) as u32 / span) + 1)
            .min(w.div_ceil(span))
            .saturating_sub((bounds.min.x.max(0.0) as u32 / span).min(w.saturating_sub(1) / span));
        let rows = ((bounds.max.y.max(0.0).min(h as f32) as u32 / span) + 1)
            .min(h.div_ceil(span))
            .saturating_sub((bounds.min.y.max(0.0) as u32 / span).min(h.saturating_sub(1) / span));
        if columns as u64 * rows as u64 * TILE as u64 * TILE as u64 * 4 <= GPU_BUDGET as u64 / 2
            || level == 16
        {
            return level;
        }
        level += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_preload_is_promoted_and_its_small_frame_can_draw_before_the_full_reply() {
        let folder = tempfile::tempdir().unwrap();
        let mut viewer = browser_ui::tests::viewer(["a.png", "b.png"].into_iter().map(|name| Entry {
            name: name.into(), size: 8, directory: false, archive: false, modified: None,
        }).collect());
        viewer.source = Some(folder.path().to_owned());
        viewer.browser = false;
        viewer.selected = Some(0);
        viewer.schedule_preload();
        let task = viewer.pending.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let (preview_tx, preview_rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        viewer.running = Some(Running { id: task.id, cancel: cancel.clone(), receiver,
            preview: Some((1, preview_rx)), preload: Some((1, false)), promoted: false });
        viewer.load_index(1, false);
        assert!(viewer.pending.is_none() && viewer.running.as_ref().unwrap().promoted);
        assert_eq!(viewer.serial, task.id);
        assert!(!cancel.load(Ordering::Relaxed));

        let rgba = vec![65, 77, 90, 255];
        preview_tx.send(worker::ContinuityPreview::sample(1, 1, &rgba, 1, 1).unwrap()).ok().unwrap();
        let ctx = egui::Context::default();
        let frame = ctx.run(egui::RawInput::default(), |ctx| {
            viewer.poll(ctx);
            egui::CentralPanel::default().show(ctx, |ui| viewer.draw_image(ui, ctx));
        });
        assert!(viewer.current.is_none());
        assert!(frame.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text.contains("b.png · 低清预览"))));

        viewer.extra.compare = 1; // Leave the next speculative task undispatched.
        sender.send(Event { id: task.id, request: task.request, operation: task.operation,
            result: Ok(worker::Output {
                reply: worker::Reply { width: 1, height: 1, preview_width: 1, preview_height: 1, ..Default::default() },
                rgba: rgba.clone(), preview: rgba.clone(),
            }),
        }).ok().unwrap();
        let _ = ctx.run(egui::RawInput::default(), |ctx| viewer.poll(ctx));
        assert_eq!(viewer.current_key, Some((1, false)));
        assert_eq!(viewer.current.as_ref().unwrap().rgba, rgba);
    }

    #[test]
    fn exit_and_native_close_wait_for_the_read_task_to_retire() {
        for native in [false, true] {
            let mut viewer = browser_ui::tests::viewer(vec![Entry {
                name: "a.png".into(), size: 1, directory: false, archive: false, modified: None,
            }]);
            viewer.source = Some("test.zip".into());
            viewer.properties = true; // Closing must not dispatch a fresh metadata request.
            let request = Request::List { path: "test.zip".into(), password: None };
            let operation = Operation::List { path: "test.zip".into(), select: None,
                password: None, management: false };
            let (sender, receiver) = mpsc::sync_channel(1);
            let cancel = Arc::new(AtomicBool::new(false));
            viewer.running = Some(Running { id: 1, cancel: cancel.clone(), receiver,
                preview: None, preload: None, promoted: false });
            viewer.serial = 2;
            viewer.pending = Some(Task { id: 2, request: request.clone(), operation: operation.clone() });
            let ctx = egui::Context::default();
            let mut input = egui::RawInput::default();
            if native { input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close); }
            let output = ctx.run(input, |ctx| {
                if native { assert!(viewer.handle_close_request(ctx)); }
                else { viewer.perform(Action::Exit, ctx); }
            });
            let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
            assert!(commands.iter().any(|c| matches!(c, egui::ViewportCommand::CancelClose)));
            assert!(!commands.iter().any(|c| matches!(c, egui::ViewportCommand::Close)));
            assert!(viewer.closing && cancel.load(Ordering::Relaxed));
            assert!(viewer.pending.is_none());
            assert!(sender.send(Event { id: 1, request, operation, result: Err("已取消".into()) }).is_ok());
            let output = ctx.run(egui::RawInput::default(), |ctx| assert!(viewer.handle_close_request(ctx)));
            assert!(viewer.running.is_none() && viewer.pending.is_none());
            assert!(output.viewport_output[&egui::ViewportId::ROOT].commands.iter()
                .any(|c| matches!(c, egui::ViewportCommand::Close)));
            assert!(!output.viewport_output[&egui::ViewportId::ROOT].commands.iter()
                .any(|c| matches!(c, egui::ViewportCommand::CancelClose)));
        }
    }

    #[test]
    fn idle_native_close_never_cancels_its_own_close_token() {
        for already_closing in [false, true] {
            let mut viewer = browser_ui::tests::viewer(Vec::new());
            viewer.closing = already_closing;
            let ctx = egui::Context::default();
            let mut input = egui::RawInput::default();
            input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close);
            let output = ctx.run(input, |ctx| assert!(viewer.handle_close_request(ctx)));
            let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
            assert!(commands.iter().any(|c| matches!(c, egui::ViewportCommand::Close)));
            assert!(!commands.iter().any(|c| matches!(c, egui::ViewportCommand::CancelClose)));
        }
    }

    #[test]
    fn a_pending_write_still_blocks_window_exit() {
        let mut viewer = browser_ui::tests::viewer(Vec::new());
        viewer.extra.pending_write = Some(Box::new(|| Ok(String::new())));
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| viewer.perform(Action::Exit, ctx));
        assert!(!viewer.closing && viewer.extra.pending_write.is_some());
        assert!(!output.viewport_output[&egui::ViewportId::ROOT].commands.iter()
            .any(|c| matches!(c, egui::ViewportCommand::Close)));
    }

    #[test]
    fn fast_navigation_and_ready_before_paint_never_return_to_the_welcome_screen() {
        let dir = tempfile::tempdir().unwrap();
        let mut viewer = browser_ui::tests::viewer(["a.png", "b.png", "c.png"].into_iter().map(|name| Entry {
            name: name.into(), size: 8, directory: false, archive: false, modified: None,
        }).collect());
        viewer.source = Some(dir.path().to_owned());
        viewer.browser = false;
        let image = || Arc::new(Pixels {
            width: 2, height: 1, rgba: vec![255; 8], preview: vec![255; 8],
            preview_width: 2, preview_height: 1, note: String::new(), record: None,
        });
        let ctx = egui::Context::default();
        let draw = |viewer: &mut Viewer| {
            ctx.run(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                ..Default::default()
            }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| viewer.draw_image(ui, ctx));
            })
        };
        viewer.set_current(0, false, image());
        let _ = draw(&mut viewer);
        assert!(viewer.extra.presentation.has_frame());
        viewer.set_current(1, false, image()); // Ready, but no frame containing B was painted.
        for index in [2, 1, 2, 0, 1] {
            viewer.load_index(index, false);
            assert!(viewer.current.is_none() && viewer.current_key.is_none());
            viewer.error = Some("暂时读取失败".into());
            viewer.password_dialog = true;
            let output = draw(&mut viewer);
            assert!(output.shapes.iter().any(|shape| matches!(shape.shape, egui::Shape::Mesh(_))));
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.job.text == "RustView")));
            assert!(viewer.extra.presentation.has_frame());
            viewer.password_dialog = false;
        }
        viewer.perform(Action::Close, &ctx);
        assert!(!viewer.extra.presentation.has_frame());
    }

    #[test]
    fn transparency_backdrop_stays_fixed_when_zoomed_and_rotated() {
        let ctx = egui::Context::default();
        let mut viewer = Viewer::empty();
        let row = [255, 64, 32, 0, 255, 64, 32, 128, 255, 64, 32, 255];
        viewer.current = Some(Arc::new(Pixels {
            width: 3, height: 2, rgba: row.repeat(2), preview: row.repeat(2),
            preview_width: 3, preview_height: 2, note: String::new(), record: None,
        }));
        let draw = |viewer: &mut Viewer| {
            ctx.run(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                ..Default::default()
            }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| viewer.draw_image(ui, ctx));
            })
        };
        let first = draw(&mut viewer);
        let meshes = |output: &egui::FullOutput| {
            output.shapes.iter().filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => Some((mesh.texture_id, mesh.calc_bounds(), shape.clip_rect)),
                _ => None,
            }).collect::<Vec<_>>()
        };
        let first_meshes = meshes(&first);
        let [(checker, backdrop, _), (overview, photo, _)] = first_meshes.as_slice() else {
            panic!("expected one checkerboard followed by one image");
        };
        assert_eq!(backdrop, photo);
        let upload = |id| &first.textures_delta.set.iter().find(|(t, _)| *t == id).unwrap().1;
        assert_eq!(upload(*checker).image.size(), [2, 2]);
        assert_eq!(upload(*checker).options, egui::TextureOptions::NEAREST_REPEAT);
        let egui::ImageData::Color(pattern) = &upload(*checker).image else {
            panic!("expected checkerboard colors");
        };
        assert_eq!(pattern.pixels, [
            Color32::from_gray(224), Color32::from_gray(176),
            Color32::from_gray(176), Color32::from_gray(224),
        ]);
        let egui::ImageData::Color(pixels) = &upload(*overview).image else {
            panic!("expected RGBA image");
        };
        assert_eq!(pixels.pixels[..3].iter().map(|c| c.a()).collect::<Vec<_>>(), [0, 128, 255]);

        viewer.fit = false;
        viewer.rotation = 1;
        viewer.scale = 64.0;
        viewer.target_scale = 64.0;
        viewer.pan.x = viewer.extra.viewport.width() / 2.0;
        let zoomed = draw(&mut viewer);
        let zoomed_meshes = meshes(&zoomed);
        let [(zoom_checker, zoom_backdrop, _), (detail, zoom_photo, clip)] = zoomed_meshes.as_slice() else {
            panic!("expected one checkerboard followed by one detail tile");
        };
        assert_eq!(zoom_checker, checker);
        assert_ne!(detail, overview);
        assert_eq!(zoom_photo.size(), egui::vec2(128.0, 192.0));
        assert_eq!(*zoom_backdrop, zoom_photo.intersect(*clip));
        for output in [&first, &zoomed] {
            let mesh = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == *checker => Some(mesh),
                _ => None,
            }).expect("checkerboard mesh");
            for vertex in &mesh.vertices {
                assert_eq!(vertex.uv.to_vec2() * 16.0, vertex.pos.to_vec2());
            }
        }
    }

    #[test]
    fn view_arrow_consumption_keeps_modified_and_release_events() {
        let event = |key, modifiers, pressed| egui::Event::Key {
            key, physical_key: None, modifiers, pressed, repeat: false,
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput {
            events: vec![
                event(egui::Key::ArrowUp, egui::Modifiers::NONE, true),
                event(egui::Key::ArrowLeft, egui::Modifiers::SHIFT, true),
                event(egui::Key::ArrowDown, egui::Modifiers::ALT, true),
                event(egui::Key::ArrowUp, egui::Modifiers::NONE, false),
            ],
            ..Default::default()
        }, |ctx| {
            ctx.input_mut(consume_view_arrows);
            ctx.input(|i| {
                assert_eq!(i.events.len(), 3);
                assert!(matches!(i.events[0], egui::Event::Key { modifiers, pressed: true, .. } if modifiers.shift));
                assert!(matches!(i.events[1], egui::Event::Key { modifiers, pressed: true, .. } if modifiers.alt));
                assert!(matches!(i.events[2], egui::Event::Key { pressed: false, .. }));
            });
        });
    }

    #[test]
    fn boundary_navigation_keeps_zoom_and_pending_work() {
        let mut viewer = browser_ui::tests::viewer(vec![Entry {
            name: "only.png".into(), size: 4, directory: false, archive: false, modified: None,
        }]);
        viewer.browser = false;
        viewer.scale = 2.0;
        viewer.target_scale = 2.5;
        viewer.fit = false;
        viewer.navigate(false, false);
        viewer.navigate(true, false);
        assert_eq!((viewer.scale, viewer.target_scale), (2.0, 2.5));
        assert!(!viewer.fit);
        assert!(viewer.pending.is_none());
        assert_eq!(viewer.serial, 0);
    }

    #[test]
    fn keyboard_zoom_changes_target_without_jumping_display_scale() {
        let mut viewer = browser_ui::tests::viewer(Vec::new());
        viewer.browser = false;
        viewer.scale = 0.5;
        viewer.target_scale = 0.5;
        let ctx = egui::Context::default();
        viewer.perform(Action::ZoomIn, &ctx);
        assert_eq!(viewer.scale, 0.5);
        assert_eq!(viewer.target_scale, 0.625);
        assert!(!viewer.fit);
        viewer.perform(Action::ZoomOut, &ctx);
        assert_eq!(viewer.target_scale, 0.5);
    }

    #[test]
    fn copied_image_uses_same_flip_then_rotate_as_display() {
        let mut image = pixels(8);
        let p = Arc::get_mut(&mut image).unwrap();
        p.width = 2;
        p.height = 1;
        p.rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let (w, h, rgba) = transformed_rgba(&image, 1, true, false).unwrap();
        assert_eq!((w, h), (1, 2));
        assert_eq!(rgba, [0, 255, 0, 255, 255, 0, 0, 255]);
        assert_eq!(image.rgba, [255, 0, 0, 255, 0, 255, 0, 255]);
    }
    #[test]
    fn focused_buttons_do_not_disable_global_shortcuts() {
        let ctx = egui::Context::default();
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("file-list-button")));
        assert!(ctx.wants_keyboard_input());
        assert!(shortcuts_enabled(&ctx, false));
        assert!(!shortcuts_enabled(&ctx, true));
    }
    fn pixels(bytes: usize) -> Arc<Pixels> {
        Arc::new(Pixels {
            width: (bytes / 4) as u32,
            height: 1,
            rgba: vec![0; bytes],
            preview: Vec::new(),
            preview_width: 0,
            preview_height: 0,
            note: String::new(),
            record: None,
        })
    }
    #[test]
    fn cache_budget_evicts_lru_and_protects_current() {
        let mut cache = ImageCache {
            budget: 16,
            ..Default::default()
        };
        cache.insert((0, false), pixels(8), None);
        cache.insert((1, false), pixels(8), None);
        assert!(cache.get((0, false)).is_some());
        cache.insert((2, false), pixels(8), None);
        assert!(cache.entries.contains_key(&(0, false)));
        assert!(!cache.entries.contains_key(&(1, false)));
        assert_eq!(cache.bytes, 16);
        cache.insert((3, false), pixels(12), Some((0, false)));
        assert!(cache.entries.contains_key(&(0, false)));
        assert!(!cache.entries.contains_key(&(3, false)));
        assert_eq!(cache.bytes, 8);
    }
    #[test]
    fn visible_bounds_respect_rotation() {
        let b = visible_image_bounds(
            Rect::from_center_size(Pos2::ZERO, egui::vec2(100.0, 200.0)),
            Pos2::ZERO,
            1.0,
            1,
            false,
            false,
            200,
            100,
        );
        assert!((b.width() - 200.0).abs() < 0.01);
        assert!((b.height() - 100.0).abs() < 0.01);
    }
    #[test]
    fn quarter_turns_restore() {
        let v = egui::vec2(4.0, 7.0);
        assert_eq!(rotate(rotate(rotate(rotate(v, 1), 1), 1), 1), v);
    }
    #[test]
    fn big_image_at_fit_uses_bounded_lod() {
        let bounds = Rect::from_min_max(Pos2::ZERO, egui::pos2(15800.0, 7900.0));
        let level = tile_level(3840.0 / 15800.0, bounds, 15800, 7900);
        let span = TILE * (1u32 << level);
        assert!(level >= 2);
        assert!(
            15800u64.div_ceil(span as u64)
                * 7900u64.div_ceil(span as u64)
                * TILE as u64
                * TILE as u64
                * 4
                <= GPU_BUDGET as u64 / 2
        );
    }
    #[test]
    fn refreshing_keeps_management_or_view_mode_even_with_a_selected_filename() {
        let directory = tempfile::tempdir().unwrap();
        let mut viewer = browser_ui::tests::viewer(vec![Entry {
            name: "renamed.jpg".into(),
            size: 1,
            directory: false,
            archive: false,
            modified: None,
        }]);
        viewer.source = Some(directory.path().to_owned());
        viewer.selected = Some(0);
        let ctx = egui::Context::default();
        viewer.browser = true;
        viewer.perform(Action::Refresh, &ctx);
        assert!(
            matches!(&viewer.pending.as_ref().unwrap().operation,Operation::List{management:true,select:Some(name),..}if name=="renamed.jpg")
        );
        viewer.browser = false;
        viewer.perform(Action::Refresh, &ctx);
        assert!(matches!(
            &viewer.pending.as_ref().unwrap().operation,
            Operation::List {
                management: false,
                ..
            }
        ));
    }
    #[test]
    fn fit_width_and_height_use_the_rotated_dimensions() {
        let mut viewer = browser_ui::tests::viewer(vec![]);
        viewer.current = Some(Arc::new(Pixels {
            width: 400,
            height: 200,
            rgba: Vec::new(),
            preview: Vec::new(),
            preview_width: 0,
            preview_height: 0,
            note: String::new(),
            record: None,
        }));
        viewer.rotation = 1;
        viewer.extra.viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        viewer.legacy_command(2300, &ctx);
        assert_eq!(viewer.target_scale, 4.0);
        viewer.legacy_command(2301, &ctx);
        assert_eq!(viewer.target_scale, 1.5);
    }
}
