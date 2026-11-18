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
    fixed: Option<FixedComparison>,
    read: Option<(u64, usize)>,
    pub linked_zoom: bool,
    pub linked_pan: bool,
}
struct FixedComparison {
    indices: Vec<usize>,
    active: usize,
    panes: usize,
    split: Option<f32>,
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
    fn dimensions(&self) -> Vec2 {
        if self.rotation % 2 == 0 {
            egui::vec2(self.width as f32, self.height as f32)
        } else {
            egui::vec2(self.height as f32, self.width as f32)
        }
    }
    fn copy_view(&mut self, reference: &Self) {
        self.scale = reference.scale;
        self.target = reference.target;
        self.pan = reference.pan;
        self.fit = reference.fit;
        self.zoom_anchor = reference.zoom_anchor;
        self.rotation = reference.rotation;
        self.flip_x = reference.flip_x;
        self.flip_y = reference.flip_y;
        self.last_interaction = reference.last_interaction;
    }
    fn paint(&mut self, painter: &egui::Painter, rect: Rect, exposure_warning: bool) {
        let center = rect.center() + self.pan;
        let geometry = Pixels {
            width: self.width,
            height: self.height,
            rgba: Vec::new(),
            preview: Vec::new(),
            preview_width: 0,
            preview_height: 0,
            note: String::new(),
            record: None,
            svg_size: None,
        };
        let full =
            Rect::from_min_max(Pos2::ZERO, egui::pos2(self.width as f32, self.height as f32));
        let uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
        if self.texture.is_some() || self.detail.is_some() {
            super::paint_transparency(
                painter,
                Rect::from_center_size(center, self.dimensions() * self.scale),
            );
        }
        if let Some(t) = &self.texture {
            let hole = self.detail.as_ref().map(|(_, [x, y, w, h])| {
                Rect::from_min_size(
                    egui::pos2(*x as f32, *y as f32),
                    egui::vec2(*w as f32, *h as f32),
                )
            });
            for coarse in coarse_regions(full, hole) {
                let texture_uv = Rect::from_min_max(
                    egui::pos2(
                        coarse.min.x / self.width as f32,
                        coarse.min.y / self.height as f32,
                    ),
                    egui::pos2(
                        coarse.max.x / self.width as f32,
                        coarse.max.y / self.height as f32,
                    ),
                );
                paint_quad(
                    painter,
                    t.id(),
                    coarse,
                    texture_uv,
                    &geometry,
                    center,
                    self.scale,
                    self.rotation,
                    self.flip_x,
                    self.flip_y,
                );
            }
        }
        if let Some((t, [x, y, w, h])) = &self.detail {
            paint_quad(
                painter,
                t.id(),
                Rect::from_min_size(
                    egui::pos2(*x as f32, *y as f32),
                    egui::vec2(*w as f32, *h as f32),
                ),
                uv,
                &geometry,
                center,
                self.scale,
                self.rotation,
                self.flip_x,
                self.flip_y,
            );
        }
        if let Some(p) = self.pixels.as_ref().filter(|_| exposure_warning) {
            if self.mask.is_none() {
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
                self.mask = Some(painter.ctx().load_texture(
                    "comparison-exposure-warning",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [p.width as usize, p.height as usize],
                        &rgba,
                    ),
                    egui::TextureOptions::NEAREST,
                ));
            }
            if let Some(t) = &self.mask {
                paint_quad(
                    painter,
                    t.id(),
                    full,
                    uv,
                    &geometry,
                    center,
                    self.scale,
                    self.rotation,
                    self.flip_x,
                    self.flip_y,
                );
            }
        }
    }
    fn detail_region(&self, view: Rect, ppp: f32) -> Option<[u32; 4]> {
        let preview_scale = self.pixels.as_ref()?.width as f32 / self.width as f32;
        if self.scale * ppp <= preview_scale * 1.2
            || self.last_interaction.elapsed() <= Duration::from_millis(140)
            || (self.scale / self.target - 1.0).abs() >= 0.001
        {
            return None;
        }
        let bounds = visible_image_bounds(
            view,
            view.center() + self.pan,
            self.scale,
            self.rotation,
            self.flip_x,
            self.flip_y,
            self.width,
            self.height,
        )
        .intersect(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(self.width as f32, self.height as f32),
        ));
        if !bounds.is_positive() {
            return None;
        }
        let x = (bounds.min.x.floor() as u32).min(self.width - 1);
        let y = (bounds.min.y.floor() as u32).min(self.height - 1);
        let region = [
            x,
            y,
            (bounds.max.x.ceil() as u32).saturating_sub(x).clamp(1, self.width - x),
            (bounds.max.y.ceil() as u32).saturating_sub(y).clamp(1, self.height - y),
        ];
        (self.requested_region != Some(region)).then_some(region)
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

fn paint_comparison_divider(painter: &egui::Painter, rect: Rect, x: f32, ppp: f32) {
    let vertical = [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())];
    painter.line_segment(vertical, egui::Stroke::new(3.0 / ppp, Color32::from_black_alpha(160)));
    painter.line_segment(vertical, egui::Stroke::new(1.0 / ppp, Color32::from_white_alpha(210)));

    let grip = Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(23.0, 16.0));
    painter.rect_filled(grip, 4.0, Color32::from_black_alpha(190));
    for offset in [-3.0, 3.0] {
        painter.line_segment(
            [
                egui::pos2(x + offset, grip.top() + 4.0),
                egui::pos2(x + offset, grip.bottom() - 4.0),
            ],
            egui::Stroke::new(1.0 / ppp, Color32::from_white_alpha(210)),
        );
    }

    let font = FontId::proportional(12.0);
    if x - rect.left() > 25.0 {
        painter.text(rect.left_top() + egui::vec2(8.0, 7.0), egui::Align2::LEFT_TOP, "A", font.clone(), Color32::WHITE);
    }
    if rect.right() - x > 25.0 {
        painter.text(rect.right_top() + egui::vec2(-8.0, 7.0), egui::Align2::RIGHT_TOP, "B", font, Color32::WHITE);
    }
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
    pub(super) fn is_fixed_reference(&self) -> bool {
        self.fixed.is_some()
    }
    pub(super) fn track_read(&mut self, id: u64, slot: usize) {
        self.read = Some((id, slot));
    }
    fn reference_index(&self) -> Option<usize> {
        self.fixed.as_ref().map(|_| self.slots[0].index)
    }
    fn shown_slot(&self, show_reference: bool) -> usize {
        if show_reference || self.slots[1].texture.is_none() && self.slots[0].texture.is_some() {
            0
        } else {
            1
        }
    }
    fn sync_fixed_view(&mut self, view_changed: bool) {
        let (a, b) = self.slots.split_at_mut(1);
        b[0].copy_view(&a[0]);
        if view_changed {
            b[0].requested_region = None;
        }
    }
    fn navigation_target(&self, action: Action) -> Option<usize> {
        let current = self.active_index()?;
        let position = self.pool.iter().position(|&i| i == current)?;
        let reference = self.reference_index();
        let candidate = |&&index: &&usize| Some(index) != reference;
        match action {
            Action::Next => self.pool[position + 1..].iter().find(candidate),
            Action::Previous => self.pool[..position].iter().rev().find(candidate),
            Action::First => self.pool.iter().find(candidate),
            Action::Last => self.pool.iter().rev().find(candidate),
            _ => None,
        }
        .copied()
    }
    fn enter_fixed(&mut self, panes: usize) {
        let mut slots = std::mem::take(&mut self.slots);
        self.fixed = Some(FixedComparison {
            indices: slots.iter().map(|s| s.index).collect(),
            active: self.active,
            panes,
            split: None,
        });
        let a = slots.remove(self.active);
        let position = self.pool.iter().position(|&i| i == a.index).unwrap_or(0);
        let index = self.pool[position + 1..]
            .iter()
            .chain(self.pool[..position].iter().rev())
            .copied()
            .find(|&i| i != a.index)
            .expect("fixed comparison requires two candidates");
        let b = slots.iter().position(|s| s.index == index).map_or_else(
            || Slot::new(index, self.generation),
            |i| slots.remove(i),
        );
        self.slots = vec![a, b];
        self.active = 1;
        self.sync_fixed_view(true);
    }
    fn exit_fixed(&mut self) -> usize {
        let layout = self.fixed.take().expect("comparison is fixed");
        let mut slots = std::mem::take(&mut self.slots);
        self.slots = layout
            .indices
            .into_iter()
            .map(|index| {
                slots.iter().position(|s| s.index == index).map_or_else(
                    || Slot::new(index, self.generation),
                    |i| slots.remove(i),
                )
            })
            .collect();
        self.active = layout.active;
        layout.panes
    }
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
            ..Comparison::default()
        };
        self.extra.compare = self.extra.comparison.slots.len().min(4);
        self.browser = false;
        self.current = None;
        self.current_key = None;
        self.pyramid = None;
        self.svg = Default::default();
        self.embedded_psd_preview = false;
        self.overview = None;
        self.tiles.clear();
        self.tile_bytes = 0;
        self.cache.clear();
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
        self.extra.comparison.read = None;
        let Some(s) = self
            .extra
            .comparison
            .slots
            .get_mut(slot)
            .filter(|s| s.generation == generation && region.is_none_or(|r| s.requested_region == Some(r)))
        else {
            return;
        };
        if let Some(region) = region {
            let size = [output.reply.width as usize, output.reply.height as usize];
            let texture = ctx.load_texture(
                format!("compare-detail-{slot}-{generation}"),
                egui::ColorImage::from_rgba_unmultiplied(size, &output.rgba),
                egui::TextureOptions::LINEAR,
            );
            s.detail = Some((texture, region));
            return;
        }
        let image = Arc::new(Pixels {
            width: output.reply.width,
            height: output.reply.height,
            rgba: output.rgba,
            preview: Vec::new(),
            preview_width: 0,
            preview_height: 0,
            note: output.reply.note,
            record: output.reply.editing_record.map(Arc::new),
            svg_size: None,
        });
        let texture = ctx.load_texture(
            format!("compare-{slot}-{generation}"),
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width as usize, image.height as usize],
                &image.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        s.width = output.reply.source_width.max(image.width);
        s.height = output.reply.source_height.max(image.height);
        s.pixels = Some(image);
        s.texture = Some(texture);
        s.error = None;
        if slot == self.extra.comparison.active {
            self.comparison_active(slot);
        }
        self.status = if self.extra.comparison.is_fixed_reference() {
            "固定 A/B 比较 · ←/→ 切换 B · 滚轮缩放 · 拖动平移"
        } else {
            "比较图像已读取；滚轮缩放，拖动平移，可切换联动"
        }
        .into();
    }
    pub(super) fn comparison_error(&mut self, slot: usize, message: String) {
        self.extra.comparison.read = None;
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
    fn cancel_comparison_read(&mut self, only_slot: Option<usize>) {
        let c = &mut self.extra.comparison;
        let Some((id, slot)) = c.read.filter(|&(_, slot)| only_slot.is_none_or(|i| i == slot)) else {
            return;
        };
        c.read = None;
        if let Some(s) = c.slots.get_mut(slot) {
            s.requested = false;
            s.requested_region = None;
        }
        if self.pending.as_ref().is_some_and(|p| p.id == id) {
            self.pending = None;
        }
        if let Some(read) = self.running.as_ref().filter(|r| r.id == id) {
            read.cancel.store(true, Ordering::Relaxed);
        }
        if id == self.serial {
            self.serial += 1;
        }
    }
    fn toggle_fixed_comparison(&mut self) {
        self.cancel_comparison_read(None);
        let c = &mut self.extra.comparison;
        if c.is_fixed_reference() {
            self.extra.compare = c.exit_fixed();
        } else {
            c.enter_fixed(self.extra.compare);
            self.extra.compare = c.slots.len();
        }
        self.comparison_active(self.extra.comparison.active);
    }
    fn comparison_replace(&mut self, index: usize) {
        if self.extra.comparison.is_fixed_reference() {
            let c = &self.extra.comparison;
            if c.reference_index() == Some(index)
                || c.active_index() == Some(index)
                    && (c.slots[1].pixels.is_some() || self.running.is_some() || self.pending.is_some())
            {
                return;
            }
            // Preserve A's first preview. A detail can wait until the new B is visible.
            let only_slot = c.slots[0].pixels.is_none().then_some(1);
            self.cancel_comparison_read(only_slot);
            let c = &mut self.extra.comparison;
            c.generation = c.generation.wrapping_add(1);
            c.slots[1] = Slot::new(index, c.generation);
            c.sync_fixed_view(false);
            self.selected = Some(index);
            self.current = None;
            self.current_key = None;
            return;
        }
        self.cancel_comparison_read(None);
        let c = &mut self.extra.comparison;
        c.generation = c.generation.wrapping_add(1);
        if let Some(s) = c.slots.get_mut(c.active) {
            *s = Slot::new(index, c.generation);
        }
        self.current = None;
        self.current_key = None;
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
                        if self.extra.comparison.is_fixed_reference() {
                            self.cancel_comparison_read(None);
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
                if c.is_fixed_reference() {
                    if let Some(index) = c.navigation_target(action) {
                        self.comparison_replace(index);
                    }
                } else if let Some(index) = c.active_index() {
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
                let c = &mut self.extra.comparison;
                let fixed = c.is_fixed_reference();
                let owner = if fixed { 0 } else { c.active };
                let linked = !fixed && c.linked_zoom;
                for (i, s) in c.slots.iter_mut().enumerate() {
                    if i != owner && !linked {
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
                if fixed {
                    c.sync_fixed_view(true);
                }
                let active = c.active;
                self.comparison_active(active);
                true
            }
            Action::LegacyCommand(id @ 2164..=2167) => {
                if !self.extra.comparison.is_fixed_reference() {
                    self.extra.compare = (id - 2163) as usize;
                }
                true
            }
            Action::LegacyCommand(2160) => {
                let c = &mut self.extra.comparison;
                if !c.is_fixed_reference() {
                    c.linked_pan = !c.linked_pan;
                }
                true
            }
            Action::LegacyCommand(2277) => {
                let c = &mut self.extra.comparison;
                if !c.is_fixed_reference() {
                    c.linked_zoom = !c.linked_zoom;
                }
                true
            }
            Action::LegacyCommand(id @ (2300 | 2301)) => {
                let c = &mut self.extra.comparison;
                let fixed = c.is_fixed_reference();
                let owner = if fixed { 0 } else { c.active };
                let linked = !fixed && c.linked_zoom;
                for (i, s) in c.slots.iter_mut().enumerate() {
                    if i == owner || linked {
                        s.fit = Some(if id == 2300 {
                            FitMode::Width
                        } else {
                            FitMode::Height
                        });
                        s.zoom_anchor = None;
                        s.pan = Vec2::ZERO;
                    }
                }
                if fixed {
                    c.slots[0].requested_region = None;
                    c.sync_fixed_view(true);
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
        let mut toggle_fixed = false;
        let mut show_reference = false;
        ui.horizontal(|ui| {
            if ui.button("完成比较").clicked() {
                close = true;
            }
            let c = &mut self.extra.comparison;
            if let Some(fixed) = &mut c.fixed {
                toggle_fixed = ui.button("返回并排比较").clicked();
                let hold = ui.add_enabled(
                    c.slots[0].texture.is_some(),
                    egui::Button::new("按住看参考图 A"),
                );
                show_reference = hold.is_pointer_button_down_on()
                    && ctx.input(|i| i.focused && i.pointer.primary_down());
                if ui.selectable_label(fixed.split.is_some(), "分割对照").clicked() {
                    fixed.split = fixed.split.is_none().then_some(0.5);
                }
            } else {
                for n in 1..=4 {
                    ui.selectable_value(&mut self.extra.compare, n, format!("{n}图"));
                }
                ui.checkbox(&mut c.linked_zoom, "联动缩放");
                ui.checkbox(&mut c.linked_pan, "联动平移");
                toggle_fixed = ui
                    .add_enabled(c.pool.len() > 1, egui::Button::new("固定参考图 A"))
                    .clicked();
            }
        });
        if close {
            self.close_comparison();
            return;
        }
        if toggle_fixed {
            self.toggle_fixed_comparison();
            ctx.request_repaint();
        }
        let fixed = self.extra.comparison.is_fixed_reference();
        let panes = if fixed { 1 } else { self.extra.compare };
        let total = self.extra.comparison.pool.len();
        let layout = egui::Layout::left_to_right(egui::Align::Min);
        ui.allocate_ui_with_layout(ui.available_size(), layout, |ui| {
            let mut draw_candidates = |ui: &mut egui::Ui| {
                egui::ScrollArea::vertical()
                    .id_salt("comparison-list")
                    .max_width(180.0)
                    .auto_shrink([!fixed, true])
                    .show_rows(ui, 26.0, total, |ui, range| {
                        if fixed {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        }
                        for p in range {
                            let c = &self.extra.comparison;
                            let index = c.pool[p];
                            if let Some(entry) = self.entries.get(index) {
                                let selected = c.active_index() == Some(index);
                                let response = if fixed {
                                    let size = [ui.available_width(), 26.0];
                                    if c.reference_index() == Some(index) {
                                        ui.add_sized(size, egui::Label::new(format!("A · {}", entry.name)))
                                    } else {
                                        let label: egui::WidgetText = if selected {
                                            format!("B · {}", entry.name).into()
                                        } else {
                                            entry.name.as_str().into()
                                        };
                                        ui.add_sized(size, egui::SelectableLabel::new(selected, label))
                                    }
                                    .on_hover_text(&entry.name)
                                } else {
                                    ui.selectable_label(selected, &entry.name)
                                };
                                if response.clicked() {
                                    replace = Some(index);
                                }
                            }
                        }
                    });
            };
            if fixed {
                ui.allocate_ui_with_layout(
                    egui::vec2(180.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    draw_candidates,
                );
            } else {
                draw_candidates(ui);
            }
            let available = ui.available_rect_before_wrap();
            let columns = if panes == 1 { 1 } else { 2 };
            let rows = panes.div_ceil(columns);
            let cell = egui::vec2(
                available.width() / columns as f32,
                available.height() / rows as f32,
            );
            compare_cell = cell;
            let scroll = ctx.input(|i| i.raw_scroll_delta.y);
            let ppp = ctx.pixels_per_point();
            let now = Instant::now();
            let exposure_warning = self.extra.exposure_warning;
            let c = &mut self.extra.comparison;
            for pane in 0..panes {
                let rect = Rect::from_min_size(
                    available.min
                        + egui::vec2(
                            (pane % columns) as f32 * cell.x,
                            (pane / columns) as f32 * cell.y,
                        ),
                    cell,
                )
                .shrink(4.0);
                let response = ui.interact(
                    rect,
                    ui.make_persistent_id(("compare-slot", pane)),
                    Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 2.0, Color32::from_gray(22));
                let split_ready = fixed && !show_reference && rect.is_positive()
                    && c.slots.iter().all(|slot| slot.texture.is_some());
                let split = c.fixed.as_mut()
                    .filter(|_| split_ready)
                    .and_then(|fixed| fixed.split.as_mut())
                    .map(|fraction| {
                        let x = rect.left() + *fraction * rect.width();
                        let handle = Rect::from_min_max(
                            egui::pos2(x - 9.0, rect.top()),
                            egui::pos2(x + 9.0, rect.bottom()),
                        ).intersect(rect);
                        let divider = ui.interact(
                            handle, ui.make_persistent_id("compare-split"), Sense::drag(),
                        ).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                        if divider.dragged_by(egui::PointerButton::Primary) {
                            *fraction = (*fraction + divider.drag_delta().x / rect.width()).clamp(0.0, 1.0);
                        }
                        let edge = painter.round_to_pixel(rect.left() + *fraction * rect.width());
                        (edge.clamp(rect.left(), rect.right()), divider)
                    });
                let Some(s) = c.slots.get_mut(pane) else {
                    continue;
                };
                let pointer = response.hover_pos()
                    .or_else(|| split.as_ref().and_then(|(_, divider)| divider.hover_pos()));
                let zoomed = pointer.is_some() && scroll != 0.0;
                let dragged = response.dragged();
                if !fixed && (response.clicked() || dragged || zoomed) {
                    active = Some(pane);
                }
                if let Some(mode) = s.fit {
                    s.scale = comparison_fit(mode, rect.size(), s.dimensions(), ppp);
                    s.target = s.scale;
                }
                if zoomed {
                    let mouse = pointer.unwrap_or(rect.center());
                    let anchor = (mouse - rect.center() - s.pan) / s.scale;
                    let target = (s.target * (scroll * 0.0025).exp()).clamp(0.0001, 64.0);
                    s.zoom_anchor = Some((mouse - rect.center(), anchor));
                    s.target = target;
                    s.fit = None;
                    s.last_interaction = now;
                    s.requested_region = None;
                    sync = Some((pane, target, s.pan / rect.size(), true));
                }
                if dragged {
                    s.zoom_anchor = None;
                    s.pan += response.drag_delta();
                    s.fit = None;
                    s.last_interaction = now;
                    s.requested_region = None;
                    sync = Some((pane, s.target, s.pan / rect.size(), false));
                }
                if response.double_clicked() {
                    s.fit = Some(FitMode::Inside);
                    s.zoom_anchor = None;
                    s.pan = Vec2::ZERO;
                    if fixed {
                        s.last_interaction = now;
                        s.requested_region = None;
                        ctx.request_repaint();
                    }
                }
                s.advance_zoom(ctx.input(|i| i.stable_dt));
                if (s.scale / s.target - 1.0).abs() > 0.0001 {
                    ctx.request_repaint();
                }
                let number = if fixed {
                    c.sync_fixed_view(zoomed || dragged || response.double_clicked());
                    detail = [1, 0].into_iter().find_map(|i| {
                        let s = &c.slots[i];
                        s.detail_region(rect, ppp)
                            .map(|r| (i, s.index, s.generation, r))
                    });
                    c.shown_slot(show_reference)
                } else {
                    pane
                };
                if let Some((x, _)) = split {
                    let left = Rect::from_min_max(rect.min, egui::pos2(x, rect.bottom()));
                    let right = Rect::from_min_max(egui::pos2(x, rect.top()), rect.max);
                    for (slot, clip) in c.slots.iter_mut().zip([left, right]) {
                        if clip.is_positive() {
                            slot.paint(&painter.with_clip_rect(clip), rect, exposure_warning);
                        }
                    }
                    paint_comparison_divider(&painter, rect, x, ppp);
                    continue;
                }
                let s = &mut c.slots[number];
                s.paint(&painter, rect, exposure_warning);
                if s.pixels.is_some() {
                    if !fixed && number == c.active {
                        detail = s
                            .detail_region(rect, ppp)
                            .map(|r| (number, s.index, s.generation, r));
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
                if !fixed && number == c.active {
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
                let prefix = if fixed {
                    if number == 0 { "A · " } else { "B · " }
                } else {
                    ""
                };
                let label = if fixed && s.texture.is_none() {
                    format!("{prefix}{label}")
                } else {
                    format!("{prefix}{label} · {}×{}", s.width, s.height)
                };
                painter.text(
                    rect.left_bottom() - egui::vec2(0.0, 6.0),
                    egui::Align2::LEFT_BOTTOM,
                    label,
                    egui::TextStyle::Small.resolve(ui.style()),
                    Color32::WHITE,
                );
                if fixed && number == 0 && c.slots[1].texture.is_none() {
                    let b = &c.slots[1];
                    let name = self.entries.get(b.index).map_or("", |entry| &entry.name);
                    let state = b.error.as_deref().unwrap_or("正在读取…");
                    painter.text(
                        rect.left_top() + egui::vec2(6.0, 6.0),
                        egui::Align2::LEFT_TOP,
                        format!("B · {name} · {state}"),
                        egui::TextStyle::Small.resolve(ui.style()),
                        Color32::WHITE,
                    );
                }
            }
            ui.allocate_rect(available, Sense::hover());
        });
        if let Some((source, scale, pan, zoom)) = sync.filter(|_| !fixed) {
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
            ctx.request_repaint();
        }
        if let Some((slot, index, generation, region)) = detail {
            let target_ready = !fixed || {
                let b = &self.extra.comparison.slots[1];
                b.pixels.is_some() || b.error.is_some()
            };
            if self.running.is_none()
                && self.pending.is_none()
                && target_ready
                && self.extra.comparison.slots.get(slot).is_some_and(|s| s.generation == generation)
                && (!fixed || !self.password_dialog && !self.closing)
            {
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
            ..Comparison::default()
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
