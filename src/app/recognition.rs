use super::{theme, visible_image_bounds, Operation, Pixels, Viewer};
use crate::{ocr, qr};
use egui::{Context, PointerButton, Pos2, Rect};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc, Weak,
    },
    time::{Duration, Instant},
};

mod text_selection;
use text_selection::{Layout, Transform};

const QR_DWELL: Duration = Duration::from_millis(350);
const TEXT_DWELL: Duration = Duration::from_millis(650);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    AutoQr,
    AutoText,
    Qr,
    Ocr,
}

#[derive(Clone, Copy)]
struct Input {
    mode: Mode,
    region: Option<Rect>,
    rotation: u8,
    flip_x: bool,
    flip_y: bool,
}

enum Output {
    Qr(Vec<qr::Hit>),
    Ocr(Result<String, String>),
    Text(Option<Layout>),
}

struct Task {
    mode: Mode,
    cancel: Arc<AtomicBool>,
    reply: Receiver<Output>,
}

enum Panel {
    Qr { scanning: bool },
    Ocr(Option<Result<String, String>>),
}

#[derive(Default)]
pub(super) struct State {
    picture: Option<Weak<Pixels>>,
    qr_at: Option<Instant>,
    text_at: Option<Instant>,
    task: Option<Task>,
    pending: Option<Input>,
    selection: Option<(Mode, Option<Pos2>)>,
    hits: Vec<qr::Hit>,
    panel: Option<Panel>,
    layout: Option<Layout>,
    copied_until: Option<Instant>,
    pub(super) menu_open: bool,
}

impl State {
    pub(super) fn reset(&mut self) {
        self.stop();
        self.picture = None;
        self.qr_at = None;
        self.text_at = None;
        self.layout = None;
        self.hits.clear();
        self.panel = None;
        self.menu_open = false;
    }

    fn stop(&mut self) {
        self.defer_auto_text();
        if let Some(task) = &self.task {
            task.cancel.store(true, Ordering::Relaxed);
        }
        self.pending = None;
        self.selection = None;
        if let Some(layout) = &mut self.layout {
            layout.clear();
        }
        self.copied_until = None;
    }

    fn defer_auto_text(&mut self) {
        if let Some(task) = &self.task {
            if task.mode == Mode::AutoText && !task.cancel.swap(true, Ordering::Relaxed) {
                self.text_at = Some(Instant::now() + TEXT_DWELL);
            }
        }
    }

    pub(super) fn selecting(&self) -> bool {
        self.selection.is_some()
    }

    pub(super) fn dragging_text(&self) -> bool {
        self.layout.as_ref().is_some_and(Layout::dragging)
    }

    pub(super) fn handle_keys(&mut self, ctx: &Context) -> bool {
        if (self.selecting() || self.panel.is_some())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.stop();
            self.panel = None;
            return true;
        }
        if self.panel.is_some()
            || self.selecting()
            || self.menu_open
            || super::text_field_focused(ctx)
        {
            return false;
        }
        let Some(layout) = &mut self.layout else {
            return false;
        };
        let has_selection = layout.has_selection();
        if (has_selection || layout.dragging())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            layout.clear();
            self.copied_until = None;
            return true;
        }
        let (all, copy) = ctx.input(|i| {
            use egui::{Event, Key, Modifiers};
            let plain_command = |modifiers: Modifiers| modifiers.matches_exact(Modifiers::COMMAND);
            let pressed = |key| i.events.iter().any(|event| matches!(event,
                Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && plain_command(*modifiers)));
            let all = (has_selection || layout.hovered())
                && pressed(Key::A);
            let copy = (has_selection || all) && (pressed(Key::C) || {
                let (key, modifiers) = crate::shortcuts::clipboard_modifiers(i, &[Key::C, Key::Insert]);
                key != Some(Key::Insert) && (plain_command(modifiers) || modifiers.is_none())
                    && i.events.iter().any(|event| matches!(event, Event::Copy))
            });
            (all, copy)
        });
        if all {
            layout.select_all();
        }
        if copy {
            self.copy_selected(ctx);
        }
        all || copy
    }

    fn copy_selected(&mut self, ctx: &Context) {
        if self.layout.as_ref().is_some_and(|layout| layout.copy(ctx)) {
            self.copied_until = Some(Instant::now() + Duration::from_millis(1100));
            ctx.request_repaint();
        }
    }

    pub(super) fn drained(&mut self) -> bool {
        self.poll();
        self.task.is_none()
    }

    fn is_current(&self, picture: &Arc<Pixels>) -> bool {
        self.picture
            .as_ref()
            .is_some_and(|old| old.as_ptr() == Arc::as_ptr(picture))
    }

    fn observe(&mut self, picture: &Arc<Pixels>, auto: bool) {
        if !self.is_current(picture) {
            self.reset();
            self.picture = Some(Arc::downgrade(picture));
            let now = Instant::now();
            self.qr_at = auto.then_some(now + QR_DWELL);
            self.text_at = auto.then_some(now + TEXT_DWELL);
        }
    }

    fn queue(&mut self, input: Input) {
        self.stop();
        if input.mode == Mode::Qr {
            self.qr_at = None;
            self.hits.clear();
        }
        self.panel = match input.mode {
            Mode::Qr => Some(Panel::Qr { scanning: true }),
            Mode::Ocr => Some(Panel::Ocr(None)),
            Mode::AutoQr | Mode::AutoText => unreachable!("only a manual request opens the panel"),
        };
        self.pending = Some(input);
    }

    fn poll(&mut self) {
        let Some(task) = &self.task else { return };
        let result = match task.reply.try_recv() {
            Err(TryRecvError::Empty) => return,
            result => result,
        };
        let task = self.task.take().unwrap();
        if task.cancel.load(Ordering::Relaxed) {
            return;
        }
        match result {
            Ok(Output::Qr(hits)) => {
                self.hits = hits;
                if task.mode == Mode::Qr {
                    self.panel = Some(Panel::Qr { scanning: false });
                }
            }
            Ok(Output::Ocr(text)) => self.panel = Some(Panel::Ocr(Some(text))),
            Ok(Output::Text(layout)) => self.layout = layout,
            Err(_) if task.mode == Mode::Ocr => {
                self.panel = Some(Panel::Ocr(Some(Err("文字识别任务异常退出".into()))));
            }
            Err(_) if task.mode == Mode::Qr => self.panel = Some(Panel::Qr { scanning: false }),
            Err(_) => {}
        }
    }

    fn run(&mut self, ctx: &Context, allow_qr: bool, allow_text: bool) {
        self.poll();
        if self.task.is_some() {
            return;
        }
        let input = if let Some(input) = self.pending.take() {
            input
        } else {
            if self.selecting() || self.panel.is_some() {
                return;
            }
            let next = [
                (allow_qr, &mut self.qr_at, Mode::AutoQr),
                (allow_text, &mut self.text_at, Mode::AutoText),
            ]
            .into_iter()
            .filter_map(|(allowed, deadline, mode)| {
                Some(((*deadline).filter(|_| allowed)?, deadline, mode))
            })
            .min_by_key(|(at, _, _)| *at);
            let Some((at, deadline, mode)) = next else {
                return;
            };
            if let Some(delay) = at.checked_duration_since(Instant::now()) {
                ctx.request_repaint_after(delay);
                return;
            }
            *deadline = None;
            Input {
                mode,
                region: None,
                rotation: 0,
                flip_x: false,
                flip_y: false,
            }
        };
        let Some(picture) = self.picture.clone() else {
            return;
        };
        let (sender, reply) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel.clone();
        let context = ctx.clone();
        let mode = input.mode;
        let thread = std::thread::Builder::new().name("rustview-recognition".into());
        match thread.spawn(move || {
            #[cfg(windows)]
            if matches!(mode, Mode::AutoQr | Mode::AutoText) {
                use windows_sys::Win32::System::Threading::{
                    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
                };
                unsafe {
                    SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
                }
            }
            let pixels = picture.upgrade().and_then(|picture| {
                let source = egui::vec2(picture.width as f32, picture.height as f32);
                gray_sample(&picture, input, &cancel_worker).map(|sample| (sample, source))
            });
            if !cancel_worker.load(Ordering::Relaxed) {
                let output = match (mode, pixels) {
                    (Mode::Ocr, Some((sample, _))) => Output::Ocr(ocr::recognize(
                        sample.width,
                        sample.height,
                        &sample.gray,
                        sample.light_on_transparent,
                        &cancel_worker,
                    )),
                    (Mode::Ocr, None) => Output::Ocr(Err("选区没有可识别的图像".into())),
                    (Mode::AutoText, Some((sample, source))) => Output::Text(
                        ocr::recognize_layout(
                            sample.width,
                            sample.height,
                            &sample.gray,
                            sample.light_on_transparent,
                            &cancel_worker,
                        )
                        .ok()
                        .map(|lines| Layout::new(lines, sample.width, sample.height, source))
                        .filter(|layout| !layout.is_empty()),
                    ),
                    (Mode::AutoText, None) => Output::Text(None),
                    (_, Some((sample, _))) => Output::Qr(qr::scan(
                        sample.width,
                        sample.height,
                        &sample.gray,
                        &cancel_worker,
                    )),
                    (_, None) => Output::Qr(Vec::new()),
                };
                let _ = sender.send(output);
            }
            drop(sender);
            context.request_repaint();
        }) {
            Ok(_) => {
                self.task = Some(Task {
                    mode,
                    cancel,
                    reply,
                })
            }
            Err(error) if mode == Mode::Ocr => {
                self.panel = Some(Panel::Ocr(Some(Err(format!("无法启动文字识别：{error}")))))
            }
            Err(_) if mode == Mode::Qr => self.panel = Some(Panel::Qr { scanning: false }),
            Err(_) => {}
        }
    }
}

impl Viewer {
    pub(super) fn recognition_blocked(&self) -> bool {
        self.browser
            || self.pyramid.is_some()
            || self.extra.compare != 0
            || self.extra.edit_dialog.is_some()
            || self.startup_image.is_some()
            || self.password_dialog
            || self.extra.dialog.is_some()
            || self.archive_jump.open
            || self.recent_ui.open
            || self.properties
            || self.extra.settings_ui.open
            || self.extra.print_ui.open
            || self.help
            || self.error.is_some()
            || self
                .pending
                .as_ref()
                .is_some_and(|task| matches!(task.operation, Operation::List { .. }))
            || self.extra.animated && !self.extra.animation_paused && !self.extra.animation_ended
    }

    fn text_transform(&self, image: &Pixels, center: Pos2) -> Transform {
        Transform {
            center,
            size: egui::vec2(image.width as f32, image.height as f32),
            scale: self.scale,
            rotation: self.rotation,
            flip_x: self.flip_x,
            flip_y: self.flip_y,
        }
    }

    pub(super) fn interact_image_text(
        &mut self,
        response: &egui::Response,
        image: &Arc<Pixels>,
        center: Pos2,
    ) -> bool {
        if self.recognition.layout.is_none()
            || self.recognition_blocked()
            || self.recognition.selecting()
            || self.recognition.panel.is_some()
            || self.recognition.menu_open
            || self.selected != self.current_key.map(|key| key.0)
            || !self.recognition.is_current(image)
        {
            return false;
        }
        let transform = self.text_transform(image, center);
        self.recognition
            .layout
            .as_mut()
            .is_some_and(|layout| layout.interact(response, transform))
    }

    pub(super) fn recognition_image(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        image: &Arc<Pixels>,
        viewport: Rect,
        center: Pos2,
        dimensions: egui::Vec2,
    ) {
        if self.recognition_blocked() || self.selected != self.current_key.map(|key| key.0) {
            return;
        }
        self.recognition.observe(image, !self.extra.animated);
        let bounds = Rect::from_center_size(center, dimensions * self.scale).intersect(viewport);
        if !bounds.is_positive() {
            return;
        }
        let input = Input {
            mode: Mode::Ocr,
            region: None,
            rotation: self.rotation,
            flip_x: self.flip_x,
            flip_y: self.flip_y,
        };
        let within_image = response
            .interact_pointer_pos()
            .is_some_and(|p| bounds.contains(p));
        if response.context_menu_opened() || response.secondary_clicked() && within_image {
            response.context_menu(|ui| {
                if self
                    .recognition
                    .layout
                    .as_ref()
                    .is_some_and(Layout::has_selection)
                    && ui.button("复制所选文字").clicked()
                {
                    self.recognition.copy_selected(ui.ctx());
                    ui.close_menu();
                }
                if ui.button("识别整图文字（OCR）").clicked() {
                    self.recognition.queue(input);
                    ui.close_menu();
                }
                for (label, mode) in [("框选文字（OCR）…", Mode::Ocr), ("框选二维码…", Mode::Qr)]
                {
                    if ui.button(label).clicked() {
                        self.recognition.stop();
                        self.recognition.panel = None;
                        self.recognition.selection = Some((mode, None));
                        ui.close_menu();
                    }
                }
            });
        }
        self.recognition.menu_open = response.context_menu_opened();
        let transform = self.text_transform(image, center);
        if !self.recognition.menu_open
            && self.recognition.panel.is_none()
            && !self.recognition.selecting()
        {
            if let Some(layout) = &mut self.recognition.layout {
                layout.paint(response, painter, transform);
            }
        }
        let Some((mode, origin)) = &mut self.recognition.selection else {
            return;
        };
        let ctx = response.ctx.clone();
        if response.hovered() || origin.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if response.drag_started_by(PointerButton::Primary) {
            *origin = ctx
                .input(|i| i.pointer.press_origin())
                .filter(|p| bounds.contains(*p));
        }
        let selection = origin
            .zip(ctx.input(|i| i.pointer.interact_pos()))
            .map(|(a, b)| Rect::from_two_pos(a, b.clamp(bounds.min, bounds.max)));
        painter.text(
            viewport.center_top() + egui::vec2(0.0, 20.0),
            egui::Align2::CENTER_TOP,
            "拖动框选区域 · Esc 取消",
            egui::FontId::proportional(14.0),
            theme::TEXT,
        );
        if let Some(selection) = selection {
            painter.rect_filled(selection, 0.0, egui::Color32::from_white_alpha(25));
            painter.rect_stroke(
                selection,
                0.0,
                egui::Stroke::new(1.0_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            if let Some(selection) = selection.filter(|r| r.width() >= 3.0 && r.height() >= 3.0) {
                let mode = *mode;
                let region = visible_image_bounds(
                    selection,
                    center,
                    self.scale,
                    self.rotation,
                    self.flip_x,
                    self.flip_y,
                    image.width,
                    image.height,
                );
                self.recognition.queue(Input {
                    mode,
                    region: Some(region),
                    ..input
                });
            } else {
                *origin = None;
            }
        }
    }

    pub(super) fn draw_recognition(&mut self, ctx: &Context) {
        if self.recognition_blocked()
            || self.current.is_none()
            || self.selected != self.current_key.map(|key| key.0)
        {
            self.recognition.reset();
            self.recognition.poll();
            return;
        }
        let quiet = ctx.input(|i| {
            i.focused
                && !i.pointer.any_down()
                && !i.key_down(egui::Key::ArrowLeft)
                && !i.key_down(egui::Key::ArrowRight)
        });
        let idle = self.directory_scan.is_idle()
            && self
                .running
                .as_ref()
                .is_none_or(|task| task.preload.is_some());
        let allow_qr = quiet && idle && !self.recognition.menu_open;
        let allow_text = allow_qr
            && self.running.is_none()
            && self.pending.is_none()
            && self.deferred_image.is_none()
            && (self.scale / self.target_scale - 1.0).abs() <= 0.0001
            && ctx.input(|i| i.raw_scroll_delta == egui::Vec2::ZERO);
        if !allow_text {
            self.recognition.defer_auto_text();
        }
        self.recognition.run(ctx, allow_qr, allow_text);
        if let Some(delay) = self
            .recognition
            .copied_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
        {
            ctx.request_repaint_after(delay);
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("picture-text-hint"),
            ));
            let label = painter.layout_no_wrap(
                "已复制所选文字".to_owned(),
                egui::FontId::proportional(13.0),
                theme::TEXT,
            );
            let pos = self.extra.viewport.center_bottom()
                - egui::vec2(label.size().x * 0.5, label.size().y + 12.0);
            painter.rect_filled(
                Rect::from_min_size(pos, label.size()).expand2(egui::vec2(9.0, 5.0)),
                6.0,
                egui::Color32::from_black_alpha(155),
            );
            painter.galley(pos, label, theme::TEXT);
        }
        if !self.recognition.hits.is_empty()
            && !self.recognition.selecting()
            && self.recognition.panel.is_none()
        {
            egui::Area::new(egui::Id::new("qr-recognition-chip"))
                .anchor(
                    egui::Align2::RIGHT_BOTTOM,
                    egui::vec2(
                        -12.0,
                        self.extra.viewport.bottom() - ctx.screen_rect().bottom() - 38.0,
                    ),
                )
                .movable(false)
                .show(ctx, |ui| {
                    let label = format!("二维码 {}", self.recognition.hits.len());
                    if ui
                        .button(label)
                        .on_hover_text("查看当前图片的二维码")
                        .clicked()
                    {
                        self.recognition.panel = Some(Panel::Qr { scanning: false });
                    }
                });
        }
        let Some(panel) = &self.recognition.panel else {
            return;
        };
        let mut open = true;
        let mut cancel = false;
        let title = match panel {
            Panel::Qr { .. } => "二维码",
            Panel::Ocr(_) => "图片文字识别",
        };
        egui::Window::new(title)
            .id(egui::Id::new("picture-recognition-result"))
            .open(&mut open)
            .default_width(420.0)
            .collapsible(false)
            .show(ctx, |ui| match panel {
                Panel::Ocr(None) | Panel::Qr { scanning: true } => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("正在本机识别…");
                        cancel = ui.button("取消").clicked();
                    });
                }
                Panel::Ocr(Some(Ok(text))) if text.is_empty() => {
                    ui.label("未识别到文字");
                }
                Panel::Ocr(Some(Ok(text))) => {
                    if ui.button("复制全部文字").clicked() {
                        ctx.copy_text(text.clone());
                    }
                    selectable_text(ui, text, 300.0);
                }
                Panel::Ocr(Some(Err(error))) => {
                    ui.label(error);
                }
                Panel::Qr { scanning: false } if self.recognition.hits.is_empty() => {
                    ui.label("未识别到二维码");
                }
                Panel::Qr { scanning: false } => {
                    egui::ScrollArea::vertical()
                        .max_height(410.0)
                        .show(ui, |ui| {
                            for (index, hit) in self.recognition.hits.iter().enumerate() {
                                if index != 0 {
                                    ui.separator();
                                }
                                selectable_text(ui, &hit.text, 120.0);
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button("复制内容").clicked() {
                                        ctx.copy_text(hit.text.clone());
                                    }
                                    if let Some(url) = &hit.url {
                                        if ui
                                            .button(format!(
                                                "打开 {}",
                                                url.host_str().unwrap_or_default()
                                            ))
                                            .clicked()
                                        {
                                            ctx.open_url(egui::OpenUrl::new_tab(url.as_str()));
                                        }
                                    }
                                });
                            }
                        });
                }
            });
        if !open || cancel {
            self.recognition.stop();
            self.recognition.panel = None;
        }
    }
}

fn selectable_text(ui: &mut egui::Ui, text: &str, height: f32) {
    egui::ScrollArea::vertical()
        .max_height(height)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut &*text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2),
            );
        });
}

#[derive(Debug, PartialEq, Eq)]
struct GraySample {
    width: u32,
    height: u32,
    gray: Vec<u8>,
    light_on_transparent: bool,
}

fn luma(pixel: &[u8]) -> u32 {
    (u32::from(pixel[0]) * 77 + u32::from(pixel[1]) * 150 + u32::from(pixel[2]) * 29 + 128) >> 8
}

fn light_on_transparent(
    rgba: &[u8],
    stride: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> bool {
    let step = width.max(height).div_ceil(128) as usize;
    let mut pixels = 0;
    let mut translucent = 0;
    let mut brightness = 0_u64;
    let mut weight = 0_u64;
    for row in (y..y + height).step_by(step) {
        for col in (x..x + width).step_by(step) {
            let offset = (row as usize * stride + col as usize) * 4;
            let pixel = &rgba[offset..offset + 4];
            let alpha = u64::from(pixel[3]);
            pixels += 1;
            translucent += u32::from(alpha <= 224);
            // Discount semitransparent shadows when estimating the lettering's polarity.
            brightness += u64::from(luma(pixel)) * alpha * alpha;
            weight += alpha * alpha;
        }
    }
    translucent * 32 >= pixels && brightness > 127 * weight
}

fn gray_sample(image: &Pixels, input: Input, cancel: &AtomicBool) -> Option<GraySample> {
    let (source_w, source_h, rgba) = if input.mode == Mode::AutoQr {
        (image.preview_width, image.preview_height, &image.preview)
    } else {
        (image.width, image.height, &image.rgba)
    };
    let bytes = (source_w as usize)
        .checked_mul(source_h as usize)?
        .checked_mul(4)?;
    if bytes == 0 || bytes != rgba.len() {
        return None;
    }
    let region = input.region.unwrap_or_else(|| {
        Rect::from_min_max(Pos2::ZERO, egui::pos2(source_w as f32, source_h as f32))
    });
    let x0 = (region.min.x.floor() as u32).min(source_w);
    let y0 = (region.min.y.floor() as u32).min(source_h);
    let w = (region.max.x.ceil() as u32).min(source_w).checked_sub(x0)?;
    let h = (region.max.y.ceil() as u32).min(source_h).checked_sub(y0)?;
    if w == 0 || h == 0 {
        return None;
    }
    let light_on_transparent = matches!(input.mode, Mode::Ocr | Mode::AutoText)
        && light_on_transparent(rgba, source_w as usize, x0, y0, w, h);
    // Use a dark matte for light lettering, then send dark-on-light pixels to OCR.
    let matte = if light_on_transparent { 32 } else { 255 };

    let rotation = input.rotation % 4;
    let (oriented_w, oriented_h) = if rotation % 2 == 0 { (w, h) } else { (h, w) };
    let (side, area) = match input.mode {
        Mode::AutoQr => (1024, 1024.0 * 1024.0),
        Mode::AutoText => (1800, 2_000_000.0),
        Mode::Qr => (2048, 2048.0 * 2048.0),
        Mode::Ocr => (4096, 12_582_912.0),
    };
    let area_scale = (area / (oriented_w as f64 * oriented_h as f64)).sqrt();
    let factor = (side as f64 / oriented_w.max(oriented_h) as f64)
        .min(area_scale)
        .min(1.0);
    let width = ((oriented_w as f64 * factor).floor() as u32).max(1);
    let height = ((oriented_h as f64 * factor).floor() as u32).max(1);
    let mut gray = Vec::with_capacity(width as usize * height as usize);
    let sample = |out: u32, original: u32, resized: u32| {
        (((2 * out as u64 + 1) * original as u64) / (2 * resized as u64)) as u32
    };
    for y in 0..height {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let sy = sample(y, oriented_h, height);
        for x in 0..width {
            let sx = sample(x, oriented_w, width);
            let (mut px, mut py) = match rotation {
                1 => (sy, h - 1 - sx),
                2 => (w - 1 - sx, h - 1 - sy),
                3 => (w - 1 - sy, sx),
                _ => (sx, sy),
            };
            if input.flip_x {
                px = w - 1 - px;
            }
            if input.flip_y {
                py = h - 1 - py;
            }
            let offset = ((y0 + py) as usize * source_w as usize + (x0 + px) as usize) * 4;
            let luma = luma(&rgba[offset..offset + 4]);
            let alpha = u32::from(rgba[offset + 3]);
            let value = ((luma * alpha + matte * (255 - alpha) + 127) / 255) as u8;
            gray.push(if light_on_transparent {
                255 - value
            } else {
                value
            });
        }
    }
    Some(GraySample {
        width,
        height,
        gray,
        light_on_transparent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels() -> Arc<Pixels> {
        Arc::new(Pixels {
            width: 3,
            height: 2,
            rgba: [10, 20, 30, 40, 50, 60]
                .into_iter()
                .flat_map(|v| [v, v, v, 255])
                .collect(),
            preview: vec![0, 0, 0, 0],
            preview_width: 1,
            preview_height: 1,
            note: String::new(),
            record: None,
        })
    }

    #[test]
    fn auto_reads_preview_while_manual_crop_matches_rotated_image() {
        let image = pixels();
        let cancel = AtomicBool::new(false);
        let auto = Input {
            mode: Mode::AutoQr,
            region: None,
            rotation: 0,
            flip_x: false,
            flip_y: false,
        };
        assert_eq!(
            gray_sample(&image, auto, &cancel),
            Some(GraySample {
                width: 1,
                height: 1,
                gray: vec![255],
                light_on_transparent: false,
            })
        );
        let crop = Input {
            mode: Mode::Ocr,
            region: Some(Rect::from_min_max(Pos2::new(1.0, 0.0), Pos2::new(3.0, 2.0))),
            rotation: 1,
            flip_x: true,
            ..auto
        };
        assert_eq!(
            gray_sample(&image, crop, &cancel),
            Some(GraySample {
                width: 2,
                height: 2,
                gray: vec![60, 30, 50, 20],
                light_on_transparent: false
            })
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(gray_sample(&image, crop, &cancel).is_none());
    }

    #[test]
    fn ocr_keeps_light_lettering_visible_on_transparency() {
        let mut image = Arc::try_unwrap(pixels()).ok().unwrap();
        image.rgba = [
            [255, 255, 255, 255],
            [255, 235, 170, 255],
            [255, 255, 255, 128],
            [255, 255, 255, 0],
            [0, 0, 0, 35],
            [0, 0, 0, 0],
        ]
        .concat();
        let input = Input {
            mode: Mode::Ocr,
            region: None,
            rotation: 0,
            flip_x: false,
            flip_y: false,
        };
        let cancel = AtomicBool::new(false);
        let sample = gray_sample(&image, input, &cancel).unwrap();
        assert!(sample.light_on_transparent);
        assert_eq!(sample.gray, vec![0, 21, 111, 223, 227, 223]);
        assert_eq!(
            gray_sample(
                &image,
                Input {
                    mode: Mode::AutoText,
                    ..input
                },
                &cancel
            ),
            Some(sample)
        );
        let qr = gray_sample(
            &image,
            Input {
                mode: Mode::Qr,
                ..input
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(qr.gray[0], 255);
        assert_eq!(qr.gray[3], 255);
        for pixel in image.rgba.chunks_exact_mut(4) {
            pixel[..3].fill(0);
        }
        let sample = gray_sample(&image, input, &cancel).unwrap();
        assert!(!sample.light_on_transparent);
        assert_eq!(sample.gray, vec![0, 0, 127, 255, 220, 255]);
    }

    #[test]
    fn automatic_text_samples_full_pixels_with_a_bounded_input() {
        let mut image = Arc::try_unwrap(pixels()).ok().unwrap();
        image.width = 2400;
        image.height = 1800;
        image.rgba = [100, 100, 100, 255].repeat((image.width * image.height) as usize);
        let sample = gray_sample(
            &image,
            Input {
                mode: Mode::AutoText,
                region: None,
                rotation: 0,
                flip_x: false,
                flip_y: false,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(sample.width > 1 && sample.width <= 1800);
        assert!(sample.gray.len() <= 2_000_000);
        assert!(sample.gray.iter().all(|&value| value == 100));
    }

    #[test]
    fn a_result_from_an_old_picture_is_discarded() {
        let old = pixels();
        let next = pixels();
        let mut state = State::default();
        state.observe(&old, true);
        let (sender, reply) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        state.task = Some(Task {
            mode: Mode::Ocr,
            cancel: cancel.clone(),
            reply,
        });
        state.observe(&next, true);
        assert!(cancel.load(Ordering::Relaxed));
        sender
            .send(Output::Ocr(Ok("previous".into())))
            .ok()
            .unwrap();
        assert!(state.drained());
        assert!(state.panel.is_none());
        assert!(state.qr_at.unwrap() > Instant::now());
        assert!(state.text_at.unwrap() > Instant::now());
    }

    #[test]
    fn cancelled_automatic_text_never_replaces_the_current_layout() {
        let image = pixels();
        let mut state = State::default();
        state.observe(&image, true);
        state.text_at = None;
        let (sender, reply) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        state.task = Some(Task {
            mode: Mode::AutoText,
            cancel: cancel.clone(),
            reply,
        });
        state.defer_auto_text();
        assert!(cancel.load(Ordering::Relaxed));
        let retry = state.text_at.unwrap();
        state.defer_auto_text();
        assert_eq!(state.text_at, Some(retry));
        let old = Layout::new(Vec::new(), 3, 2, egui::vec2(3.0, 2.0));
        sender.send(Output::Text(Some(old))).ok().unwrap();
        assert!(state.drained());
        assert!(state.layout.is_none());
    }
}
