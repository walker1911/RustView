use super::*;
use std::time::Duration;

const OVERVIEW_SIDE: u32 = 1536;
#[derive(Clone, Copy)]
enum FitMode {
    Inside,
    Width,
    Height,
}
#[derive(Default)]
pub(super) struct Comparison {
    pool: Vec<usize>,
    slots: Vec<Slot>,
    active: usize,
    generation: u64,
    pub linked_zoom: bool,
    pub linked_pan: bool,
}
struct Slot {
    index: usize,
    generation: u64,
    requested: bool,
    error: Option<String>,
    pixels: Option<Arc<Pixels>>,
    texture: Option<TextureHandle>,
    mask: Option<TextureHandle>,
    width: u32,
    height: u32,
    scale: f32,
    target: f32,
    pan: Vec2,
    fit: Option<FitMode>,
    zoom_anchor: Option<(Vec2, Vec2)>,
    rotation: u8,
    flip_x: bool,
    flip_y: bool,
    detail: Option<(TextureHandle, [u32; 4])>,
    requested_region: Option<[u32; 4]>,
    last_interaction: Instant,
}
impl Slot {
    fn new(index: usize, generation: u64) -> Self {
        Self {
            index,
            generation,
            requested: false,
            error: None,
            pixels: None,
            texture: None,
            mask: None,
            width: 1,
            height: 1,
            scale: 1.0,
            target: 1.0,
            pan: Vec2::ZERO,
            fit: Some(FitMode::Inside),
            zoom_anchor: None,
            rotation: 0,
            flip_x: false,
            flip_y: false,
            detail: None,
            requested_region: None,
            last_interaction: Instant::now(),
        }
    }
    fn advance_zoom(&mut self, dt: f32) {
        let factor = 1.0 - (-18.0 * dt.clamp(0.0, 0.05)).exp();
        self.scale = (self.scale.ln() + (self.target.ln() - self.scale.ln()) * factor).exp();
        if (self.scale / self.target - 1.0).abs() < 0.0001 {
            self.scale = self.target;
        }
        if let Some((screen, image)) = self.zoom_anchor {
            self.pan = screen - image * self.scale;
            if self.scale == self.target {
                self.zoom_anchor = None;
            }
        }
    }
}
fn comparison_fit(mode: FitMode, viewport: Vec2, image: Vec2, ppp: f32) -> f32 {
    match mode {
        FitMode::Inside => (viewport.x / image.x)
            .min(viewport.y / image.y)
            .min(1.0 / ppp),
        FitMode::Width => viewport.x / image.x,
        FitMode::Height => viewport.y / image.y,
    }
    .clamp(0.0001, 64.0)
}
fn coarse_regions(full: Rect, detail: Option<Rect>) -> Vec<Rect> {
    let Some(hole) = detail
        .map(|r| r.intersect(full))
        .filter(|r| r.is_positive())
    else {
        return vec![full];
    };
    [
        Rect::from_min_max(full.min, egui::pos2(full.max.x, hole.min.y)),
        Rect::from_min_max(egui::pos2(full.min.x, hole.max.y), full.max),
        Rect::from_min_max(
            egui::pos2(full.min.x, hole.min.y),
            egui::pos2(hole.min.x, hole.max.y),
        ),
        Rect::from_min_max(
            egui::pos2(hole.max.x, hole.min.y),
            egui::pos2(full.max.x, hole.max.y),
        ),
    ]
    .into_iter()
    .filter(|r| r.is_positive())
    .collect()
}
impl Comparison {
    pub fn active_index(&self) -> Option<usize> {
        self.slots.get(self.active).map(|s| s.index)
    }
    pub fn active_dimensions(&self) -> Option<(u32, u32)> {
        self.slots.get(self.active).map(|s| (s.width, s.height))
    }
}

impl Viewer {
    pub(super) fn start_comparison(&mut self) {
        if self.extra.edited && !confirm_discard() {
            return;
        }
        let pool = self
            .selected_indices()
            .into_iter()
            .filter(|i| {
                self.entries
                    .get(*i)
                    .is_some_and(|e| !e.directory && !e.archive)
            })
            .collect::<Vec<_>>();
        if pool.is_empty() {
            self.status = "请先选择要比较的图片".into();
            return;
        }
        self.extra.edit_dialog=None;
        self.stop_animation();
        self.serial += 1;
        self.pending = None;
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.extra.edited = false;
        self.extra.edits.clear();
        self.extra.redo_edits.clear();
        let generation = self.extra.comparison.generation.wrapping_add(1);
        let slots = pool
            .iter()
            .take(4)
            .map(|i| Slot::new(*i, generation))
            .collect();
        self.extra.comparison = Comparison {
            pool,
            slots,
            active: 0,
            generation,
            linked_zoom: true,
            linked_pan: true,
        };
        self.extra.compare = self.extra.comparison.slots.len().min(4);
        self.browser = false;
        self.current = None;
        self.current_key = None;
        self.overview = None;
        self.tiles.clear();
        self.tile_bytes = 0;
        self.cache = ImageCache::default();
        self.status = "正在按需读取比较图像…".into();
    }
    pub(super) fn reset_comparison(&mut self) {
        self.extra.compare = 0;
        self.extra.comparison = Comparison::default();
    }
    pub(super) fn close_comparison(&mut self) {
        let index = self.extra.comparison.active_index();
        self.reset_comparison();
        self.serial += 1;
        self.pending = None;
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(i) = index {
            self.current = None;
            self.current_key = None;
            self.load_index(i, self.full_raw);
            self.browser = true;
        }
    }
    pub(super) fn compare_request(&self, slot: usize) -> Option<Request> {
        let s = self.extra.comparison.slots.get(slot)?;
        let source = self.make_load(s.index, self.full_raw)?;
        Some(Request::Preview {
            source: Box::new(source),
            max_side: OVERVIEW_SIDE,
            region: None,
        })
    }
    pub(super) fn schedule_comparison(&mut self) {
        if self.extra.compare == 0
            || self.running.is_some()
            || self.pending.is_some()
            || self.password_dialog
            || self.extra.writing.is_some()
            || self.extra.pending_write.is_some()
        {
            return;
        }
        let item = self
            .extra
            .comparison
            .slots
            .iter()
            .take(self.extra.compare)
            .enumerate()
            .find(|(_, s)| !s.requested && s.pixels.is_none() && s.error.is_none())
            .map(|(i, s)| (i, s.generation));
        if let Some((slot, generation)) = item {
            if let Some(request) = self.compare_request(slot) {
                self.extra.comparison.slots[slot].requested = true;
                self.queue(
                    request,
                    Operation::Compare {
                        slot,
                        generation,
                        region: None,
                    },
                );
            }
        }
    }
    pub(super) fn accept_comparison(
        &mut self,
        slot: usize,
        generation: u64,
        region: Option<[u32; 4]>,
        output: worker::Output,
        ctx: &egui::Context,
    ) {
        if self.extra.compare == 0 {
            return;
        }
        let Some(s) = self
            .extra
            .comparison
            .slots
            .get_mut(slot)
            .filter(|s| s.generation == generation)
        else {
            return;
        };
        let image = Arc::new(Pixels {
            width: output.reply.width,
            height: output.reply.height,
            rgba: output.rgba,
            preview: Vec::new(),
            preview_width: 0,
            preview_height: 0,
            note: output.reply.note,
            record: output.reply.editing_record.map(Arc::new),
        });
        let texture = ctx.load_texture(
            format!("compare-{slot}-{generation}"),
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width as usize, image.height as usize],
                &image.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        if let Some(region) = region {
            if s.requested_region == Some(region) {
                s.detail = Some((texture, region));
            }
        } else {
            s.width = output.reply.source_width.max(image.width);
            s.height = output.reply.source_height.max(image.height);
            s.pixels = Some(image.clone());
            s.texture = Some(texture);
            s.error = None;
        }
        if region.is_none() && slot == self.extra.comparison.active {
            self.comparison_active(slot);
        }
        self.status = "比较图像已读取；滚轮缩放，拖动平移，可切换联动".into();
    }
    pub(super) fn comparison_error(&mut self, slot: usize, message: String) {
        if let Some(s) = self.extra.comparison.slots.get_mut(slot) {
            s.error = Some(message);
            s.requested = false;
        }
    }
    fn comparison_active(&mut self, slot: usize) {
        let Some(s) = self.extra.comparison.slots.get(slot) else {
            return;
        };
        self.rotation = s.rotation;
        self.flip_x = s.flip_x;
        self.flip_y = s.flip_y;
        let same = self.extra.comparison.active == slot
            && self.selected == Some(s.index)
            && match (&self.current, &s.pixels) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
        if same {
            return;
        }
        self.extra.comparison.active = slot;
        if let Some(s) = self.extra.comparison.slots.get(slot) {
            self.selected = Some(s.index);
            self.current = s.pixels.clone();
            self.current_key = self.current.as_ref().map(|_| (s.index, self.full_raw));
        }
        self.read_record();
        let record = self.current.as_ref().and_then(|p| p.record.clone());
        self.sync_workspace_source(record.as_deref());
    }
    fn comparison_replace(&mut self, index: usize) {
        let c = &mut self.extra.comparison;
        c.generation = c.generation.wrapping_add(1);
        if let Some(s) = c.slots.get_mut(c.active) {
            *s = Slot::new(index, c.generation);
        }
        self.current = None;
        self.current_key = None;
        self.serial += 1;
        self.pending = None;
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }
    pub(super) fn comparison_action(&mut self, action: Action, ctx: &egui::Context) -> bool {
        if self.extra.compare == 0 {
            return false;
        }
        match action {
            Action::LegacyCommand(2528) => true,
            Action::LegacyCommand(id)
                if matches!(
                    id,
                    2043..=2046
                        | 2268
                        | 22490
                        | 22294
                        | 10420
                        | 2285
                        | 21852
                        | 21851
                        | 10576
                        | 10579
                        | 10556
                        | 21902
                        | 2356
                ) =>
            {
                self.extra.pending_edit_id = Some(id);
                self.close_comparison();
                true
            }
            Action::CopyImage => {
                if let Some(s) = self
                    .extra
                    .comparison
                    .slots
                    .get(self.extra.comparison.active)
                {
                    if let Some(source) = self.make_load(s.index, self.full_raw) {
                        let recipe = s
                            .pixels
                            .as_ref()
                            .and_then(|p| p.record.as_ref())
                            .map(|r| r.recipe.clone())
                            .unwrap_or_default();
                        let mut edits = recipe.edits;
                        if s.flip_x {
                            edits.push(crate::editing::Edit::FlipH);
                        }
                        if s.flip_y {
                            edits.push(crate::editing::Edit::FlipV);
                        }
                        if s.rotation != 0 {
                            edits.push(crate::editing::Edit::Rotate90 { turns: s.rotation });
                        }
                        self.queue(
                            Request::Render {
                                source: Box::new(source),
                                edits,
                                settings: recipe.export,
                                raw: recipe.raw,
                                save: None,
                            },
                            Operation::CopyImage,
                        );
                    }
                }
                true
            }
            Action::ToggleBrowser | Action::Close => {
                self.close_comparison();
                true
            }
            Action::Next | Action::Previous | Action::First | Action::Last => {
                let c = &self.extra.comparison;
                if let Some(index) = c.active_index() {
                    let p = c.pool.iter().position(|i| *i == index).unwrap_or(0);
                    let n = match action {
                        Action::Next => (p + 1).min(c.pool.len() - 1),
                        Action::Previous => p.saturating_sub(1),
                        Action::First => 0,
                        _ => c.pool.len() - 1,
                    };
                    let i = c.pool[n];
                    self.comparison_replace(i);
                }
                true
            }
            Action::ZoomIn
            | Action::ZoomOut
            | Action::Fit
            | Action::ActualSize
            | Action::RotateLeft
            | Action::RotateRight
            | Action::FlipHorizontal
            | Action::FlipVertical => {
                let linked = self.extra.comparison.linked_zoom;
                let active = self.extra.comparison.active;
                for (i, s) in self.extra.comparison.slots.iter_mut().enumerate() {
                    if i != active && !linked {
                        continue;
                    }
                    match action {
                        Action::Fit => {
                            s.fit = Some(FitMode::Inside);
                            s.zoom_anchor = None;
                            s.pan = Vec2::ZERO;
                        }
                        Action::ActualSize => {
                            s.fit = None;
                            s.target = 1.0 / ctx.pixels_per_point();
                            s.pan = Vec2::ZERO;
                        }
                        Action::ZoomIn => {
                            s.fit = None;
                            s.target = (s.target * 1.25).clamp(0.0001, 64.0);
                        }
                        Action::ZoomOut => {
                            s.fit = None;
                            s.target = (s.target * 0.8).clamp(0.0001, 64.0);
                        }
                        Action::RotateLeft => s.rotation = (s.rotation + 3) % 4,
                        Action::RotateRight => s.rotation = (s.rotation + 1) % 4,
                        Action::FlipHorizontal => s.flip_x = !s.flip_x,
                        Action::FlipVertical => s.flip_y = !s.flip_y,
                        _ => {}
                    }
                    s.zoom_anchor = None;
                    s.last_interaction = Instant::now();
                    s.requested_region = None;
                }
                self.comparison_active(active);
                true
            }
            Action::LegacyCommand(2164..=2167) => {
                if let Action::LegacyCommand(id) = action {
                    self.extra.compare = (id - 2163) as usize;
                }
                true
            }
            Action::LegacyCommand(2160) => {
                self.extra.comparison.linked_pan = !self.extra.comparison.linked_pan;
                true
            }
            Action::LegacyCommand(2277) => {
                self.extra.comparison.linked_zoom = !self.extra.comparison.linked_zoom;
                true
            }
            Action::LegacyCommand(id @ (2300 | 2301)) => {
                let active = self.extra.comparison.active;
                let linked = self.extra.comparison.linked_zoom;
                for (i, s) in self.extra.comparison.slots.iter_mut().enumerate() {
                    if i == active || linked {
                        s.fit = Some(if id == 2300 {
                            FitMode::Width
                        } else {
                            FitMode::Height
                        });
                        s.zoom_anchor = None;
                        s.pan = Vec2::ZERO;
                    }
                }
                true
            }
            _ => false,
        }
    }
    pub(super) fn draw_comparison(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut close = false;
        let mut replace = None;
        let mut active = None;
        let mut sync = None;
        let mut detail = None;
        let mut compare_cell = Vec2::ZERO;
        ui.horizontal(|ui| {
            if ui.button("完成比较").clicked() {
                close = true;
            }
            for n in 1..=4 {
                ui.selectable_value(&mut self.extra.compare, n, format!("{n}图"));
            }
            ui.checkbox(&mut self.extra.comparison.linked_zoom, "联动缩放");
            ui.checkbox(&mut self.extra.comparison.linked_pan, "联动平移");
        });
        let total = self.extra.comparison.pool.len();
        ui.horizontal(|ui| {
            egui::ScrollArea::vertical()
                .id_salt("comparison-list")
                .max_width(180.0)
                .show_rows(ui, 26.0, total, |ui, range| {
                    for p in range {
                        let index = self.extra.comparison.pool[p];
                        if let Some(entry) = self.entries.get(index) {
                            if ui
                                .selectable_label(
                                    self.extra.comparison.active_index() == Some(index),
                                    &entry.name,
                                )
                                .clicked()
                            {
                                replace = Some(index);
                            }
                        }
                    }
                });
            let available = ui.available_rect_before_wrap();
            let columns = if self.extra.compare <= 1 { 1 } else { 2 };
            let rows = self.extra.compare.div_ceil(columns);
            let cell = egui::vec2(
                available.width() / columns as f32,
                available.height() / rows as f32,
            );
            compare_cell = cell;
            let scroll = ctx.input(|i| i.raw_scroll_delta.y);
            let now = Instant::now();
            for number in 0..self.extra.compare {
                let rect = Rect::from_min_size(
                    available.min
                        + egui::vec2(
                            (number % columns) as f32 * cell.x,
                            (number / columns) as f32 * cell.y,
                        ),
                    cell,
                )
                .shrink(4.0);
                let response = ui.interact(
                    rect,
                    ui.make_persistent_id(("compare-slot", number)),
                    Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 2.0, Color32::from_gray(22));
                let Some(s) = self.extra.comparison.slots.get_mut(number) else {
                    continue;
                };
                if response.clicked() || response.dragged() || response.hovered() && scroll != 0.0 {
                    active = Some(number);
                }
                let dimensions = if s.rotation % 2 == 0 {
                    egui::vec2(s.width as f32, s.height as f32)
                } else {
                    egui::vec2(s.height as f32, s.width as f32)
                };
                if let Some(mode) = s.fit {
                    s.scale = comparison_fit(mode, rect.size(), dimensions, ctx.pixels_per_point());
                    s.target = s.scale;
                }
                if response.hovered() && scroll != 0.0 {
                    let mouse = response.hover_pos().unwrap_or(rect.center());
                    let anchor = (mouse - rect.center() - s.pan) / s.scale;
                    let target = (s.target * (scroll * 0.0025).exp()).clamp(0.0001, 64.0);
                    s.zoom_anchor = Some((mouse - rect.center(), anchor));
                    s.target = target;
                    s.fit = None;
                    s.last_interaction = now;
                    s.requested_region = None;
                    sync = Some((number, target, s.pan / rect.size(), true));
                }
                if response.dragged() {
                    s.zoom_anchor = None;
                    s.pan += response.drag_delta();
                    s.fit = None;
                    s.last_interaction = now;
                    s.requested_region = None;
                    sync = Some((number, s.target, s.pan / rect.size(), false));
                }
                if response.double_clicked() {
                    s.fit = Some(FitMode::Inside);
                    s.zoom_anchor = None;
                    s.pan = Vec2::ZERO;
                }
                s.advance_zoom(ctx.input(|i| i.stable_dt));
                if (s.scale / s.target - 1.0).abs() > 0.0001 {
                    ctx.request_repaint();
                }
                let center = rect.center() + s.pan;
                let geometry = Pixels {
                    width: s.width,
                    height: s.height,
                    rgba: Vec::new(),
                    preview: Vec::new(),
                    preview_width: 0,
                    preview_height: 0,
                    note: String::new(),
                    record: None,
                };
                let full =
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(s.width as f32, s.height as f32));
                let uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
                if s.texture.is_some() || s.detail.is_some() {
                    super::paint_transparency(
                        &painter,
                        Rect::from_center_size(center, dimensions * s.scale),
                    );
                }
                if let Some(t) = &s.texture {
                    let hole = s.detail.as_ref().map(|(_, [x, y, w, h])| {
                        Rect::from_min_size(
                            egui::pos2(*x as f32, *y as f32),
                            egui::vec2(*w as f32, *h as f32),
                        )
                    });
                    for coarse in coarse_regions(full, hole) {
                        let texture_uv = Rect::from_min_max(
                            egui::pos2(
                                coarse.min.x / s.width as f32,
                                coarse.min.y / s.height as f32,
                            ),
                            egui::pos2(
                                coarse.max.x / s.width as f32,
                                coarse.max.y / s.height as f32,
                            ),
                        );
                        paint_quad(
                            &painter,
                            t.id(),
                            coarse,
                            texture_uv,
                            &geometry,
                            center,
                            s.scale,
                            s.rotation,
                            s.flip_x,
                            s.flip_y,
                        );
                    }
                }
                if let Some((t, [x, y, w, h])) = &s.detail {
                    paint_quad(
                        &painter,
                        t.id(),
                        Rect::from_min_size(
                            egui::pos2(*x as f32, *y as f32),
                            egui::vec2(*w as f32, *h as f32),
                        ),
                        uv,
                        &geometry,
                        center,
                        s.scale,
                        s.rotation,
                        s.flip_x,
                        s.flip_y,
                    );
                }
                if let Some(p) = &s.pixels {
                    if self.extra.exposure_warning {
                        if s.mask.is_none() {
                            let rgba = p
                                .rgba
                                .chunks_exact(4)
                                .flat_map(|p| {
                                    if p[3] == 0 {
                                        [0, 0, 0, 0]
                                    } else if p[..3].iter().any(|c| *c >= 250) {
                                        [255, 0, 0, 160]
                                    } else if p[..3].iter().all(|c| *c <= 5) {
                                        [0, 80, 255, 160]
                                    } else {
                                        [0, 0, 0, 0]
                                    }
                                })
                                .collect::<Vec<_>>();
                            s.mask = Some(ctx.load_texture(
                                format!("compare-warning-{number}"),
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [p.width as usize, p.height as usize],
                                    &rgba,
                                ),
                                egui::TextureOptions::NEAREST,
                            ));
                        }
                        if let Some(t) = &s.mask {
                            paint_quad(
                                &painter,
                                t.id(),
                                full,
                                uv,
                                &geometry,
                                center,
                                s.scale,
                                s.rotation,
                                s.flip_x,
                                s.flip_y,
                            );
                        }
                    }
                    let preview_scale = p.width as f32 / s.width as f32;
                    if number == self.extra.comparison.active
                        && s.scale * ctx.pixels_per_point() > preview_scale * 1.2
                        && s.last_interaction.elapsed() > Duration::from_millis(140)
                        && (s.scale / s.target - 1.0).abs() < 0.001
                    {
                        let b = visible_image_bounds(
                            rect, center, s.scale, s.rotation, s.flip_x, s.flip_y, s.width,
                            s.height,
                        )
                        .intersect(full);
                        if b.is_positive() {
                            let x = (b.min.x.floor() as u32).min(s.width - 1);
                            let y = (b.min.y.floor() as u32).min(s.height - 1);
                            let region = [
                                x,
                                y,
                                (b.max.x.ceil() as u32)
                                    .saturating_sub(x)
                                    .min(s.width - x)
                                    .max(1),
                                (b.max.y.ceil() as u32)
                                    .saturating_sub(y)
                                    .min(s.height - y)
                                    .max(1),
                            ];
                            if s.requested_region != Some(region) {
                                detail = Some((number, s.index, s.generation, region));
                            }
                        }
                    }
                } else {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        s.error.as_deref().unwrap_or("正在读取…"),
                        egui::TextStyle::Body.resolve(ui.style()),
                        Color32::GRAY,
                    );
                }
                if number == self.extra.comparison.active {
                    painter.rect_stroke(
                        rect,
                        2.0,
                        egui::Stroke::new(2.0_f32, Color32::LIGHT_BLUE),
                        egui::StrokeKind::Inside,
                    );
                }
                let label = self
                    .entries
                    .get(s.index)
                    .map(|e| e.name.as_str())
                    .unwrap_or("");
                painter.text(
                    rect.left_bottom() - egui::vec2(0.0, 6.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{label} · {}×{}", s.width, s.height),
                    egui::TextStyle::Small.resolve(ui.style()),
                    Color32::WHITE,
                );
            }
            ui.allocate_rect(available, Sense::hover());
        });
        if let Some((source, scale, pan, zoom)) = sync {
            let c = &mut self.extra.comparison;
            let anchor = c
                .slots
                .get(source)
                .and_then(|s| s.zoom_anchor)
                .map(|(screen, _)| screen / (compare_cell - egui::vec2(8.0, 8.0)));
            for (i, s) in c.slots.iter_mut().enumerate() {
                if i == source {
                    continue;
                }
                if zoom && c.linked_zoom {
                    s.target = scale;
                    s.fit = None;
                    if let Some(anchor) = anchor {
                        let screen = anchor * (compare_cell - egui::vec2(8.0, 8.0));
                        s.zoom_anchor = Some((screen, (screen - s.pan) / s.scale));
                    }
                }
                if !zoom && c.linked_pan {
                    s.pan = pan * (compare_cell - egui::vec2(8.0, 8.0));
                    s.fit = None;
                    s.zoom_anchor = None;
                }
                s.last_interaction = Instant::now();
                s.requested_region = None;
            }
        }
        if let Some(slot) = active {
            self.comparison_active(slot);
        }
        if let Some(index) = replace {
            self.comparison_replace(index);
        }
        if let Some((slot, index, generation, region)) = detail {
            if self.running.is_none() && self.pending.is_none() {
                if let Some(source) = self.make_load(index, self.full_raw) {
                    self.extra.comparison.slots[slot].requested_region = Some(region);
                    self.queue(
                        Request::Preview {
                            source: Box::new(source),
                            max_side: 4096,
                            region: Some(region),
                        },
                        Operation::Compare {
                            slot,
                            generation,
                            region: Some(region),
                        },
                    );
                }
            }
        }
        if self
            .extra
            .comparison
            .slots
            .iter()
            .any(|s| s.last_interaction.elapsed() < Duration::from_millis(200))
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if close {
            self.close_comparison();
        }
    }
    pub(super) fn copy_worker_image(&mut self, output: worker::Output) {
        match arboard::Clipboard::new().and_then(|mut c| {
            c.set_image(arboard::ImageData {
                width: output.reply.width as usize,
                height: output.reply.height as usize,
                bytes: std::borrow::Cow::Owned(output.rgba),
            })
        }) {
            Ok(()) => self.status = "当前比较图像已按原始像素复制".into(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_keeps_only_four_resident_slots_for_large_selection() {
        let pool = (0..100_000).collect::<Vec<_>>();
        let c = Comparison {
            slots: pool.iter().take(4).map(|i| Slot::new(*i, 1)).collect(),
            pool,
            active: 0,
            generation: 1,
            linked_zoom: true,
            linked_pan: true,
        };
        assert_eq!(c.slots.len(), 4);
        assert_eq!(c.active_index(), Some(0));
        assert!(c.slots.iter().all(|s| s.pixels.is_none()));
    }
    #[test]
    fn smoothing_preserves_pointer_anchor_on_every_frame() {
        let mut slot = Slot::new(0, 0);
        slot.fit = None;
        slot.scale = 0.4;
        slot.target = 3.0;
        slot.pan = egui::vec2(20.0, -10.0);
        let screen = egui::vec2(190.0, 90.0);
        let image = (screen - slot.pan) / slot.scale;
        slot.zoom_anchor = Some((screen, image));
        for _ in 0..100 {
            slot.advance_zoom(1.0 / 60.0);
            assert!((slot.pan + image * slot.scale - screen).length() < 0.0001);
        }
        assert!((slot.scale - 3.0).abs() < 0.001);
    }
    #[test]
    fn fit_axes_differ_and_coarse_pixels_do_not_cover_transparent_detail() {
        let image = egui::vec2(1000.0, 500.0);
        let view = egui::vec2(400.0, 400.0);
        assert_eq!(comparison_fit(FitMode::Width, view, image, 1.0), 0.4);
        assert_eq!(comparison_fit(FitMode::Height, view, image, 1.0), 0.8);
        let full = Rect::from_min_size(Pos2::ZERO, image);
        let hole = Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(600.0, 400.0));
        let coarse = coarse_regions(full, Some(hole));
        assert!(
            (coarse.iter().map(Rect::area).sum::<f32>() + hole.area() - full.area()).abs() < 0.1
        );
        assert!(coarse.iter().all(|r| !r.intersect(hole).is_positive()));
    }
}
