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

const QR_DWELL: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    AutoQr,
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
    auto_at: Option<Instant>,
    task: Option<Task>,
    pending: Option<Input>,
    selection: Option<(Mode, Option<Pos2>)>,
    hits: Vec<qr::Hit>,
    panel: Option<Panel>,
    pub(super) menu_open: bool,
}

impl State {
    pub(super) fn reset(&mut self) {
        self.stop();
        self.picture = None;
        self.auto_at = None;
        self.hits.clear();
        self.panel = None;
        self.menu_open = false;
    }

    fn stop(&mut self) {
        if let Some(task) = &self.task {
            task.cancel.store(true, Ordering::Relaxed);
        }
        self.pending = None;
        self.selection = None;
    }

    pub(super) fn selecting(&self) -> bool {
        self.selection.is_some()
    }

    pub(super) fn handle_escape(&mut self, ctx: &Context) -> bool {
        if (self.selecting() || self.panel.is_some())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.stop();
            self.panel = None;
            return true;
        }
        false
    }

    pub(super) fn drained(&mut self) -> bool {
        self.poll();
        self.task.is_none()
    }

    fn observe(&mut self, picture: &Arc<Pixels>, auto: bool) {
        if !self
            .picture
            .as_ref()
            .is_some_and(|old| old.as_ptr() == Arc::as_ptr(picture))
        {
            self.reset();
            self.picture = Some(Arc::downgrade(picture));
            self.auto_at = auto.then(|| Instant::now() + QR_DWELL);
        }
    }

    fn queue(&mut self, input: Input) {
        self.stop();
        if input.mode == Mode::Qr {
            self.auto_at = None;
            self.hits.clear();
        }
        self.panel = Some(match input.mode {
            Mode::Qr => Panel::Qr { scanning: true },
            Mode::Ocr => Panel::Ocr(None),
            Mode::AutoQr => unreachable!("only a manual request opens the panel"),
        });
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
            Err(_) if task.mode == Mode::Ocr => {
                self.panel = Some(Panel::Ocr(Some(Err("文字识别任务异常退出".into()))));
            }
            Err(_) if task.mode == Mode::Qr => self.panel = Some(Panel::Qr { scanning: false }),
            Err(_) => {}
        }
    }

    fn run(&mut self, ctx: &Context, allow_auto: bool) {
        self.poll();
        if self.task.is_some() {
            return;
        }
        let input = if let Some(input) = self.pending.take() {
            input
        } else {
            if !allow_auto || self.selection.is_some() || self.panel.is_some() {
                return;
            }
            let Some(deadline) = self.auto_at else { return };
            if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
                ctx.request_repaint_after(delay);
                return;
            }
            self.auto_at = None;
            Input {
                mode: Mode::AutoQr,
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
            if mode == Mode::AutoQr {
                use windows_sys::Win32::System::Threading::{
                    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
                };
                unsafe {
                    SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
                }
            }
            let pixels = picture
                .upgrade()
                .and_then(|picture| gray_sample(&picture, input, &cancel_worker));
            if !cancel_worker.load(Ordering::Relaxed) {
                let output = match (mode, pixels) {
                    (Mode::Ocr, Some((w, h, gray))) => {
                        Output::Ocr(ocr::recognize(w, h, &gray, &cancel_worker))
                    }
                    (Mode::Ocr, None) => Output::Ocr(Err("选区没有可识别的图像".into())),
                    (_, Some((w, h, gray))) => Output::Qr(qr::scan(w, h, &gray, &cancel_worker)),
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
    fn recognition_blocked(&self) -> bool {
        self.browser
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
        self.recognition
            .run(ctx, quiet && idle && !self.recognition.menu_open);
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

fn gray_sample(image: &Pixels, input: Input, cancel: &AtomicBool) -> Option<(u32, u32, Vec<u8>)> {
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

    let rotation = input.rotation % 4;
    let (oriented_w, oriented_h) = if rotation % 2 == 0 { (w, h) } else { (h, w) };
    let side = match input.mode {
        Mode::AutoQr => 1024,
        Mode::Qr => 2048,
        Mode::Ocr => 4096,
    };
    let area_scale = (12_582_912.0 / (oriented_w as f64 * oriented_h as f64)).sqrt();
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
            let luma = (u32::from(rgba[offset]) * 77
                + u32::from(rgba[offset + 1]) * 150
                + u32::from(rgba[offset + 2]) * 29
                + 128)
                >> 8;
            let alpha = u32::from(rgba[offset + 3]);
            gray.push(((luma * alpha + 255 * (255 - alpha) + 127) / 255) as u8);
        }
    }
    Some((width, height, gray))
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
        assert_eq!(gray_sample(&image, auto, &cancel), Some((1, 1, vec![255])));
        let crop = Input {
            mode: Mode::Ocr,
            region: Some(Rect::from_min_max(Pos2::new(1.0, 0.0), Pos2::new(3.0, 2.0))),
            rotation: 1,
            flip_x: true,
            ..auto
        };
        assert_eq!(
            gray_sample(&image, crop, &cancel),
            Some((2, 2, vec![60, 30, 50, 20]))
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(gray_sample(&image, crop, &cancel).is_none());
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
        assert!(state.auto_at.unwrap() > Instant::now());
    }
}
