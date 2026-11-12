use super::*;
use super::theme;
use std::collections::{BTreeSet, VecDeque};

const MAX_COVERS: usize = 32;
const MAX_SIDE: usize = 128;
const MAX_RADIUS: usize = 6;

struct ThumbnailTask {
    epoch: u64,
    index: usize,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<worker::Output, String>>,
}

#[derive(Clone)]
struct CoverFrame {
    index: usize,
    rect: Rect,
    opacity: f32,
    selected: bool,
}

pub(super) struct State {
    sequence: Vec<usize>,
    positions: Vec<usize>,
    sequence_revision: u64,
    dirty: bool,
    center: Option<f64>,
    velocity: f64,
    scene_target: Option<f64>,
    last_covers: Vec<CoverFrame>,
    ghosts: Vec<CoverFrame>,
    ghost_age: f32,
    epoch: u64,
    suspended: bool,
    textures: HashMap<usize, TextureHandle>,
    previews: HashMap<usize, super::presentation::PreviewPixels>,
    recent: VecDeque<usize>,
    failed: VecDeque<usize>,
    running: Option<ThumbnailTask>,
    wanted: Vec<usize>,
    #[cfg(test)]
    sequence_builds: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            sequence: Vec::new(), positions: Vec::new(), sequence_revision: 0, dirty: true,
            center: None, velocity: 0.0, scene_target: None, last_covers: Vec::new(),
            ghosts: Vec::new(), ghost_age: 0.0, epoch: 0, suspended: false, textures: HashMap::new(), previews: HashMap::new(), recent: VecDeque::new(),
            failed: VecDeque::new(), running: None, wanted: Vec::new(),
            #[cfg(test)]
            sequence_builds: 0,
        }
    }
}

impl State {
    pub fn invalidate_sequence(&mut self) {
        self.dirty = true;
        self.wanted.clear();
    }

    fn ensure_sequence(&mut self, entries: &[Entry], visible: &[usize], selection: &BTreeSet<usize>) {
        if !self.dirty { return; }
        self.sequence.clear();
        self.sequence.extend(visible.iter().copied().filter(|index| {
            entries.get(*index).is_some_and(|e| !e.directory && !e.archive)
                && (selection.len() <= 1 || selection.contains(index))
        }));
        self.positions.clear();
        self.positions.resize(entries.len(), usize::MAX);
        for (position, &index) in self.sequence.iter().enumerate() {
            self.positions[index] = position;
        }
        self.center = None;
        self.velocity = 0.0;
        self.scene_target = None;
        self.last_covers.clear();
        self.ghosts.clear();
        self.ghost_age = 0.0;
        self.dirty = false;
        self.sequence_revision = self.sequence_revision.wrapping_add(1);
        #[cfg(test)]
        { self.sequence_builds += 1; }
    }

    fn position(&self, selected: Option<usize>) -> Option<usize> {
        self.positions.get(selected?).copied().filter(|&position| position != usize::MAX)
    }

    fn animate(&mut self, target: f64, dt: f32) -> (f64, f32, bool) {
        let dt = dt.clamp(0.0, 0.1);
        let previous = self.center.unwrap_or(target);
        let mut rebased = false;
        if self.ghosts.is_empty() && (previous - target).abs() > 12.0 {
            self.ghosts = std::mem::take(&mut self.last_covers);
            self.ghost_age = 0.0;
            self.center = Some(target - (target - previous).signum() * 1.5);
            self.velocity = 0.0;
            self.scene_target = Some(target);
            rebased = true;
        }
        if !self.ghosts.is_empty() && !rebased { self.ghost_age += dt; }
        let previous = self.center.unwrap_or(target);
        // Coalesce another far jump while this one hands over; never stack ghost layers.
        if self.ghosts.is_empty() || (previous - target).abs() <= 12.0 {
            self.scene_target = Some(target);
        }
        let scene_target = self.scene_target.unwrap_or(target);
        let (center, velocity) = spring_step(previous, self.velocity, scene_target, dt as f64);
        self.center = Some(center);
        self.velocity = velocity;
        let progress = if self.ghosts.is_empty() { 1.0 }
            else { (self.ghost_age / 0.24).clamp(0.0, 1.0) };
        let opacity = progress * progress * (3.0 - 2.0 * progress);
        if progress == 1.0 { self.ghosts.clear(); }
        (center, opacity, center != target || velocity != 0.0 || !self.ghosts.is_empty())
    }

    fn cancel(&mut self) {
        if let Some(task) = &self.running { task.cancel.store(true, Ordering::Relaxed); }
        self.wanted.clear();
    }

    fn reset_source(&mut self) {
        self.cancel();
        self.suspended = false;
        self.epoch = self.epoch.wrapping_add(1);
        self.sequence.clear();
        self.positions.clear();
        self.invalidate_sequence();
        self.center = None;
        self.velocity = 0.0;
        self.scene_target = None;
        self.last_covers.clear();
        self.ghosts.clear();
        self.ghost_age = 0.0;
        self.textures.clear();
        self.previews.clear();
        self.recent.clear();
        self.failed.clear();
        // Retain a cancelled task until its process has actually stopped.
    }

    fn texture(&mut self, index: usize) -> Option<TextureHandle> {
        let texture = self.textures.get(&index)?.clone();
        self.recent.retain(|i| *i != index);
        self.recent.push_back(index);
        Some(texture)
    }

    fn insert(&mut self, index: usize, texture: TextureHandle) {
        self.textures.insert(index, texture);
        self.recent.retain(|i| *i != index);
        self.recent.push_back(index);
        while self.recent.len() > MAX_COVERS {
            if let Some(old) = self.recent.pop_front() {
                self.textures.remove(&old);
                self.previews.remove(&old);
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let result = self.running.as_ref().and_then(|task| match task.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("缩略图任务已结束".into())),
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = result {
            let task = self.running.take().unwrap();
            if task.epoch != self.epoch || task.cancel.load(Ordering::Relaxed) { return; }
            if let Ok(output) = result {
                let width = output.reply.width as usize;
                let height = output.reply.height as usize;
                if output.reply.error.is_none() && width > 0 && height > 0
                    && width <= MAX_SIDE && height <= MAX_SIDE && output.rgba.len() == width * height * 4
                {
                    let texture = ctx.load_texture(
                        format!("cover-{}-{}", self.epoch, task.index),
                        egui::ColorImage::from_rgba_unmultiplied([width, height], &output.rgba),
                        egui::TextureOptions::LINEAR,
                    );
                    self.insert(task.index, texture);
                    if let Some(preview) = super::presentation::PreviewPixels::sample(
                        output.reply.width, output.reply.height, &output.rgba,
                        output.reply.source_width.max(output.reply.width),
                        output.reply.source_height.max(output.reply.height),
                    ) {
                        self.previews.insert(task.index, preview);
                    }
                    return;
                }
            }
            // Speculative failures never open a password dialog or replace the main error.
            self.failed.retain(|i| *i != task.index);
            self.failed.push_back(task.index);
            if self.failed.len() > MAX_COVERS { self.failed.pop_front(); }
        }
    }
}

impl Drop for State {
    fn drop(&mut self) { self.cancel(); }
}

fn spring_step(position: f64, velocity: f64, target: f64, dt: f64) -> (f64, f64) {
    // Exact critically damped response. Retargeting keeps both position and velocity.
    let omega = 22.0;
    let dt = dt.clamp(0.0, 0.1);
    if dt == 0.0 { return (position, velocity); }
    let offset = position - target;
    let decay = (-omega * dt).exp();
    let travel = (velocity + omega * offset) * dt;
    let position = target + (offset + travel) * decay;
    let velocity = (velocity - omega * travel) * decay;
    if (position - target).abs() < 0.0005 && velocity.abs() < 0.005 {
        (target, 0.0)
    } else {
        (position, velocity)
    }
}

fn cover_style(distance: f32, side: f32) -> (f32, f32) {
    let depth = distance * distance;
    (side * (0.78 + 0.22 / (1.0 + 0.65 * depth)),
        0.12 + 0.88 * (-0.16 * depth).exp())
}

fn edge_opacity(distance: f32, radius: usize) -> f32 {
    let fade = (radius as f32 + 0.5 - distance.abs()).clamp(0.0, 1.0);
    fade * fade * (3.0 - 2.0 * fade)
}

fn cover_offset(distance: f32, side: f32) -> f32 {
    (side + 8.0) * distance
}

fn paint_cover(ui: &mut egui::Ui, rect: Rect, opacity: f32,
    texture: Option<&TextureHandle>, selected: bool,
) {
    let alpha = (opacity * 255.0).round() as u8;
    let fill = if selected { theme::SELECTED } else { theme::PANEL };
    ui.painter().rect_filled(rect, 5.0, fill.gamma_multiply(opacity));
    if let Some(texture) = texture {
        let dimensions = texture.size_vec2();
        let inner = rect.shrink(4.0);
        let scale = (inner.width() / dimensions.x).min(inner.height() / dimensions.y);
        let image = Rect::from_center_size(inner.center(), dimensions * scale);
        ui.painter().image(texture.id(), image,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::from_white_alpha(alpha));
    }
    if selected {
        ui.painter().rect_stroke(
            rect,
            5.0,
            egui::Stroke::new(1.0_f32, theme::ACCENT.gamma_multiply(opacity)),
            egui::StrokeKind::Inside,
        );
    }
}

impl Viewer {
    fn ensure_browse_sequence(&mut self) {
        self.extra.coverflow.ensure_sequence(&self.entries, &self.extra.visible, &self.extra.selection);
    }

    pub(super) fn browse_entries(&mut self) -> (u64, &[usize], &[Entry]) {
        self.ensure_browse_sequence();
        let order = &self.extra.coverflow;
        (order.sequence_revision, &order.sequence, &self.entries)
    }

    pub(super) fn navigation_target(&mut self, forward: bool, end: bool) -> Option<usize> {
        self.ensure_browse_sequence();
        let order = &self.extra.coverflow;
        let last = order.sequence.len().checked_sub(1)?;
        let position = order.position(self.selected).unwrap_or(0);
        let target = match (forward, end) {
            (false, true) => 0,
            (true, true) => last,
            (false, false) => position.saturating_sub(1),
            (true, false) => (position + 1).min(last),
        };
        let index = order.sequence[target];
        (self.selected != Some(index)).then_some(index)
    }

    pub(super) fn preload_neighbors(&mut self, forward: bool) -> [Option<usize>; 2] {
        self.ensure_browse_sequence();
        let order = &self.extra.coverflow;
        let Some(position) = order.position(self.selected) else { return [None, None] };
        let next = order.sequence.get(position + 1).copied();
        let previous = position.checked_sub(1).map(|p| order.sequence[p]);
        if forward { [next, previous] } else { [previous, next] }
    }

    pub(super) fn reset_coverflow(&mut self) {
        self.extra.coverflow.reset_source();
        self.extra.presentation.begin_source();
    }
    pub(super) fn cancel_coverflow_thumbnail(&mut self) { self.extra.coverflow.cancel(); }

    pub(super) fn suspend_coverflow_thumbnails(&mut self) {
        self.extra.coverflow.suspended = true;
        self.extra.coverflow.cancel();
    }

    pub(super) fn poll_cancelled_coverflow(&mut self, ctx: &egui::Context) -> bool {
        self.extra.coverflow.cancel();
        self.extra.coverflow.poll(ctx);
        self.extra.coverflow.running.is_none()
    }

    pub(super) fn coverflow_preview(&mut self, index: usize) -> Option<TextureHandle> {
        self.extra.coverflow.texture(index)
            .or_else(|| self.extra.thumbnails.get(&index).cloned())
            .or_else(|| self.extra.presentation.texture_for(index))
    }

    pub(super) fn remember_coverflow_preview(&mut self, index: usize,
        preview: super::presentation::PreviewPixels, texture: TextureHandle,
    ) {
        self.extra.coverflow.insert(index, texture);
        self.extra.coverflow.previews.insert(index, preview);
        self.extra.coverflow.failed.retain(|i| *i != index);
    }

    pub(super) fn prioritize_coverflow_preview(&mut self, index: usize) {
        self.extra.coverflow.suspended = false;
        if let Some(task) = &self.extra.coverflow.running {
            if task.index != index || self.source.as_ref().is_some_and(|path| worker::is_archive(path)) {
                task.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.extra.coverflow.wanted.clear();
        self.extra.coverflow.wanted.push(index);
        if let Some(preview) = self.extra.coverflow.previews.get(&index).cloned() {
            self.extra.presentation.stage_preview(index, preview);
        }
    }

    fn coverflow_thumbnail_blocked(&self, ctx: &egui::Context) -> bool {
        let local = !self.source.as_ref().is_some_and(|path| worker::is_archive(path));
        let target_due = self.extra.presentation.is_waiting()
            && self.extra.presentation.target_index() == self.selected
            && local
            && (self.extra.coverflow.running.as_ref().is_some_and(|task| Some(task.index) == self.selected)
                || self.extra.animation.is_some()
                    && self.extra.presentation.waiting_elapsed() >= std::time::Duration::from_millis(150));
        self.closing || self.archive_jump.open || self.extra.coverflow.suspended || self.source.is_none() || self.browser || self.extra.compare > 0 || self.extra.edit_dialog.is_some()
            || self.password_dialog || self.extra.dialog.is_some() || self.properties
            || self.extra.settings_ui.open || self.extra.print_ui.open || self.help
            || ctx.memory(|memory| memory.any_popup_open())
            || (self.running.is_some() || self.pending.is_some()) && !target_due
            || self.extra.retiring.is_some()
            || self.extra.writing.is_some() || self.extra.pending_write.is_some()
            || self.extra.animation.is_some() && self.current_key.map(|key| key.0) != self.selected && !target_due
    }

    fn next_coverflow_thumbnail(&self) -> Option<usize> {
        if self.extra.presentation.is_waiting() {
            // Static loads publish their own small frame. Starting a second
            // decoder here would repeat work, including archive extraction.
            if self.running.is_some() || self.pending.is_some() || self.error.is_some() {
                return None;
            }
            let target = self.extra.presentation.target_index()?;
            if self.deferred_image.is_some() && self.extra.presentation.has_target_preview(target) {
                let position = self.extra.coverflow.position(Some(target))?;
                let next = if self.nav_forward { position + 1 } else { position.checked_sub(1)? };
                let candidate = *self.extra.coverflow.sequence.get(next)?;
                if !self.extra.coverflow.previews.contains_key(&candidate)
                    && !self.extra.coverflow.failed.contains(&candidate)
                    && self.make_load(candidate, false).as_ref().is_some_and(worker::is_local_jpeg_load)
                {
                    return Some(candidate);
                }
                return None;
            }
            return Some(target).filter(|index| {
                !self.large_archived_photoshop(*index)
                    && !self.extra.presentation.has_target_preview(*index)
                    && !self.extra.coverflow.textures.contains_key(index)
                    && !self.extra.coverflow.failed.contains(index)
            });
        }
        self.extra.coverflow.wanted.iter().copied().find(|index| {
            !self.large_archived_photoshop(*index)
                && !self.extra.coverflow.textures.contains_key(index)
                && !self.extra.thumbnails.contains_key(index)
                && !self.extra.coverflow.failed.contains(index)
                && !(self.current_key.is_some_and(|key| key.0 == *index)
                    && (self.overview.is_some() || self.extra.animation_texture.is_some()))
        })
    }

    pub(super) fn poll_coverflow(&mut self, ctx: &egui::Context) {
        self.extra.coverflow.poll(ctx);
        let cached: Vec<_> = self.extra.coverflow.wanted.iter().take(13).copied()
            .filter(|index| !self.extra.coverflow.textures.contains_key(index))
            .filter_map(|index| self.cache.entries.get(&(index, false)).cloned().map(|image| (index, image)))
            .collect();
        for (index, image) in cached {
            if let Some(preview) = super::presentation::PreviewPixels::sample(
                image.preview_width, image.preview_height, &image.preview, image.width, image.height,
            ) {
                let texture = ctx.load_texture(format!("cached-cover-{index}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [preview.width as usize, preview.height as usize], &preview.rgba,
                    ), egui::TextureOptions::LINEAR);
                self.remember_coverflow_preview(index, preview, texture);
            }
        }
        if self.extra.presentation.is_waiting() {
            if let Some(index) = self.extra.presentation.target_index() {
                if let Some(preview) = self.extra.coverflow.previews.get(&index).cloned() {
                    self.extra.presentation.stage_preview(index, preview);
                }
            }
        }
        if self.coverflow_thumbnail_blocked(ctx) { self.extra.coverflow.cancel(); return; }
        if self.extra.coverflow.running.is_some() { return; }
        self.ensure_overview(ctx);
        let Some(index) = self.next_coverflow_thumbnail() else { return; };
        let Some(source) = self.make_load(index, false) else { return; };
        let epoch = self.extra.coverflow.epoch;
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let context = ctx.clone();
        std::thread::spawn(move || {
            let preview = if thread_cancel.load(Ordering::Relaxed) {
                None
            } else {
                worker::scaled_jpeg_thumbnail(&source)
            };
            let result = match preview {
                Some(output) if !thread_cancel.load(Ordering::Relaxed) => Ok(output),
                _ => worker::execute(Request::Thumbnail { source: Box::new(source) }, thread_cancel),
            };
            let _ = sender.send(result);
            context.request_repaint();
        });
        self.extra.coverflow.running = Some(ThumbnailTask { epoch, index, cancel, receiver });
    }

    pub(super) fn draw_coverflow(&mut self, ctx: &egui::Context) {
        if self.browser || self.extra.compare > 0 || self.extra.edit_dialog.is_some() {
            self.extra.coverflow.cancel();
            return;
        }
        self.ensure_browse_sequence();
        let Some(position) = self.extra.coverflow.position(self.selected) else {
            self.extra.coverflow.cancel(); return;
        };
        let total = self.extra.coverflow.sequence.len();
        if total <= 1 || ctx.available_rect().height() < 170.0 {
            self.extra.coverflow.cancel(); return;
        }
        let height = (ctx.available_rect().height() * 0.24).clamp(82.0, 160.0);
        let mut clicked = None;
        let enabled = !self.password_dialog && self.extra.dialog.is_none() && !self.properties
            && !self.extra.settings_ui.open && !self.extra.print_ui.open && !self.help
            && !ctx.memory(|memory| memory.any_popup_open());
        egui::TopBottomPanel::bottom("viewer-coverflow")
            .exact_height(height)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(theme::CANVAS))
            .show(ctx, |ui| {
                let (area, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), height - 28.0), Sense::hover());
                let side = (area.height() - 8.0).min(MAX_SIDE as f32).min(area.width() * 0.4).max(24.0);
                let radius = ((area.width() - side) / (2.0 * (side + 8.0)))
                    .floor().clamp(1.0, MAX_RADIUS as f32) as usize;
                let target = position as f64;
                let dt = ctx.input(|i| i.stable_dt);
                let (center, scene_opacity, moving) = self.extra.coverflow.animate(target, dt);
                if moving { ctx.request_repaint(); }
                for ghost in self.extra.coverflow.ghosts.clone() {
                    let texture = self.coverflow_preview(ghost.index);
                    paint_cover(ui, ghost.rect, ghost.opacity * (1.0 - scene_opacity),
                        texture.as_ref(), ghost.selected);
                }
                let anchor = (center.round() as usize).min(total - 1);
                let start = anchor.saturating_sub(radius);
                let end = (anchor + radius + 1).min(total);
                let mut positions: Vec<_> = (start..end).collect();
                positions.sort_by(|a, b| ((*b as f64 - center).abs()).total_cmp(&(*a as f64 - center).abs()));
                let mut wanted = positions.clone();
                wanted.sort_by_key(|p| (p.abs_diff(position), (*p >= position) != self.nav_forward));
                self.extra.coverflow.wanted = wanted.into_iter().map(|p| self.extra.coverflow.sequence[p]).collect();
                self.extra.coverflow.last_covers.clear();
                for p in positions {
                    let index = self.extra.coverflow.sequence[p];
                    let distance = (p as f64 - center) as f32;
                    let (size, opacity) = cover_style(distance, side);
                    let middle = area.center() + egui::vec2(cover_offset(distance, side), 0.0);
                    let rect = Rect::from_center_size(middle, egui::Vec2::splat(size));
                    let current = self.selected == Some(index);
                    let main_texture = (self.current_key.is_some_and(|key| key.0 == index))
                        .then(|| self.extra.animation_texture.clone().or_else(|| self.overview.clone())).flatten();
                    let texture = main_texture.or_else(|| self.extra.coverflow.texture(index))
                        .or_else(|| self.extra.thumbnails.get(&index).cloned());
                    let opacity = opacity * edge_opacity(distance, radius);
                    self.extra.coverflow.last_covers.push(CoverFrame {
                        index, rect, opacity, selected: current,
                    });
                    paint_cover(ui, rect, opacity * scene_opacity, texture.as_ref(), current);
                    let response = ui.interact(rect.intersect(area),
                        egui::Id::new(("coverflow-item", self.extra.coverflow.epoch, index)), Sense::click());
                    if enabled && scene_opacity > 0.25 && opacity * scene_opacity > 0.03
                        && response.clicked() && !current { clicked = Some(index); }
                    if let Some(entry) = self.entries.get(index) { response.on_hover_text(&entry.name); }
                }
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(format!("{} / {}", position + 1, total))
                        .small().strong().color(theme::TEXT));
                    if let Some(index) = self.selected {
                        if let Some(entry) = self.entries.get(index) {
                            ui.add(egui::Label::new(egui::RichText::new(&entry.name)
                                .small().color(theme::MUTED)).truncate())
                                .on_hover_text(&entry.name);
                        }
                    }
                });
            });
        if let Some(index) = clicked {
            self.cancel_coverflow_thumbnail();
            self.load_index(index, false);
        } else if !self.coverflow_thumbnail_blocked(ctx) && self.extra.coverflow.running.is_none()
            && self.next_coverflow_thumbnail().is_some()
        {
            // Requests were discovered after poll; wake exactly the next dispatch frame.
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_cancel_prevents_thumbnail_reads_until_another_target_is_selected() {
        let mut viewer = super::super::browser_ui::tests::viewer(vec![entry("a.png"), entry("b.png")]);
        viewer.source = Some("photos.zip".into());
        viewer.browser = false;
        viewer.selected = Some(0);
        viewer.extra.presentation.begin_target(0, "a.png".into());
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        viewer.extra.coverflow.running = Some(ThumbnailTask {
            epoch: 0, index: 1, cancel: cancel.clone(), receiver,
        });
        viewer.cancel_current_read();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(!viewer.extra.presentation.is_waiting());
        assert!(sender.send(Err("已取消".into())).is_ok());
        let ctx = egui::Context::default();
        viewer.extra.coverflow.poll(&ctx);
        assert!(viewer.coverflow_thumbnail_blocked(&ctx));
        viewer.prioritize_coverflow_preview(1);
        assert!(!viewer.coverflow_thumbnail_blocked(&ctx));
    }

    #[test]
    fn window_close_also_waits_for_a_cancelled_thumbnail_task() {
        let mut viewer = super::super::browser_ui::tests::viewer(Vec::new());
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        viewer.extra.coverflow.running = Some(ThumbnailTask {
            epoch: 0, index: 0, cancel: cancel.clone(), receiver,
        });
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            viewer.request_close(ctx);
            assert!(viewer.handle_close_request(ctx));
        });
        assert!(cancel.load(Ordering::Relaxed));
        assert!(!output.viewport_output[&egui::ViewportId::ROOT].commands.iter()
            .any(|c| matches!(c, egui::ViewportCommand::Close)));
        assert!(sender.send(Err("已取消".into())).is_ok());
        let output = ctx.run(egui::RawInput::default(), |ctx| assert!(viewer.handle_close_request(ctx)));
        assert!(viewer.extra.coverflow.running.is_none());
        assert!(output.viewport_output[&egui::ViewportId::ROOT].commands.iter()
            .any(|c| matches!(c, egui::ViewportCommand::Close)));
    }

    fn entry(name: &str) -> Entry {
        Entry { name: name.into(), size: 0, directory: false, archive: false, modified: None }
    }

    #[test]
    fn large_archived_photoshop_does_not_duplicate_the_foreground_spool() {
        let mut large = entry("large.PSD");
        large.size = worker::MAX_INPUT_BYTES + 1;
        let mut viewer = super::super::browser_ui::tests::viewer(vec![large, entry("small.png")]);
        viewer.source = Some("photos.7z".into());
        viewer.selected = Some(0);
        viewer.extra.presentation.begin_target(0, "large.PSD".into());
        viewer.extra.coverflow.wanted = vec![0, 1];
        assert_eq!(viewer.next_coverflow_thumbnail(), None);
        viewer.extra.presentation.ready(0);
        assert_eq!(viewer.next_coverflow_thumbnail(), Some(1));
        assert!(viewer.make_load(0, false).is_some());
    }

    #[test]
    fn spring_duration_is_consistent_across_frame_rates_and_large_indices() {
        let simulate = |fps: f64| {
            let (mut position, mut velocity, mut elapsed) = (0.0, 0.0, 0.0);
            while elapsed < 0.25 {
                let dt = (1.0 / fps).min(0.25 - elapsed);
                (position, velocity) = spring_step(position, velocity, 1.0, dt);
                elapsed += dt;
            }
            (position, velocity)
        };
        let reference = simulate(60.0);
        for fps in [30.0, 120.0, 144.0] {
            let result = simulate(fps);
            assert!((result.0 - reference.0).abs() < 1e-10);
            assert!((result.1 - reference.1).abs() < 1e-9);
            let distance = (1.0 - result.0) as f32;
            let reference_distance = (1.0 - reference.0) as f32;
            assert_eq!(cover_offset(distance, 128.0), cover_offset(reference_distance, 128.0));
            assert_eq!(cover_style(distance, 128.0), cover_style(reference_distance, 128.0));
        }
        let (position, _) = spring_step(99_998.0, 0.0, 99_999.0, 1.0 / 240.0);
        assert!(position > 99_998.0 && position < 99_998.005);
    }

    #[test]
    fn retargeting_preserves_motion_and_settles_to_an_idle_state() {
        let mut state = State::default();
        state.center = Some(0.0);
        for _ in 0..8 { state.animate(1.0, 1.0 / 120.0); }
        let (position, velocity) = (state.center.unwrap(), state.velocity);
        assert!(velocity > 0.0);
        let (same_position, _, moving) = state.animate(-1.0, 0.0);
        assert_eq!(same_position, position);
        assert_eq!(state.velocity, velocity);
        assert!(moving);
        let (next, _, _) = state.animate(-1.0, 0.000001);
        assert!(((next - position) / 0.000001 - velocity).abs() < 0.01);
        for _ in 0..150 { state.animate(-1.0, 1.0 / 120.0); }
        for _ in 0..10 {
            assert_eq!(state.animate(-1.0, 1.0 / 60.0), (-1.0, 1.0, false));
            assert_eq!(state.velocity, 0.0);
        }
    }

    #[test]
    fn distant_jumps_crossfade_one_bounded_old_neighborhood() {
        let cover = CoverFrame { index: 0, rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(128.0, 128.0)),
            opacity: 1.0, selected: true };
        let mut state = State::default();
        state.center = Some(0.0);
        state.last_covers = vec![cover.clone(); 13];
        let (_, opacity, moving) = state.animate(99_999.0, 1.0 / 60.0);
        assert_eq!(opacity, 0.0); // Old frame is intact; the rebased new scene is invisible.
        assert!(moving && state.ghosts.len() == 13);
        state.last_covers = vec![cover.clone(); 13];
        let (_, opacity, _) = state.animate(199_999.0, 1.0 / 60.0);
        assert!(opacity > 0.0 && opacity < 1.0);
        assert!(state.center.unwrap() < 100_010.0); // Coalesce instead of stacking another transition.
        for _ in 0..90 {
            state.last_covers = vec![cover.clone(); 13];
            let (_, opacity, _) = state.animate(199_999.0, 1.0 / 60.0);
            assert!(state.ghosts.len() <= 13 && state.last_covers.len() <= 13);
            assert!((0.0..=1.0).contains(&opacity));
        }
        assert!(state.ghosts.is_empty());
        assert_eq!(state.center, Some(199_999.0));
    }

    #[test]
    fn cover_edges_enter_invisibly_and_center_curves_have_no_cusp() {
        for radius in [1, 3, MAX_RADIUS] {
            assert_eq!(edge_opacity(radius as f32 + 0.5, radius), 0.0);
            assert!(edge_opacity(radius as f32 + 0.499, radius) < 0.00001);
            assert_eq!(edge_opacity(0.0, radius), 1.0);
        }
        let center = cover_style(0.0, 128.0);
        let nearby = cover_style(0.0001, 128.0);
        assert_eq!(nearby, cover_style(-0.0001, 128.0));
        assert!((center.0 - nearby.0).abs() < 0.0001);
        assert!((center.1 - nearby.1).abs() < 0.0001);
        assert_eq!(cover_offset(0.0, 128.0), 0.0);
        assert_eq!(cover_offset(0.8, 128.0), -cover_offset(-0.8, 128.0));
    }

    #[test]
    fn current_cover_reuses_ready_preview_without_a_second_decode() {
        let dir = tempfile::tempdir().unwrap();
        let mut viewer = super::super::browser_ui::tests::viewer(vec![entry("photo.NEF")]);
        viewer.browser = false;
        viewer.source = Some(dir.path().to_owned());
        viewer.current_key = Some((0, false));
        viewer.current = Some(Arc::new(Pixels {
            width: 2, height: 1, rgba: vec![255; 8],
            preview: vec![255; 8], preview_width: 2, preview_height: 1,
            note: String::new(), record: None,
        }));
        viewer.extra.coverflow.wanted = vec![0];
        let ctx = egui::Context::default();
        viewer.poll_coverflow(&ctx);
        let texture = viewer.overview.as_ref().unwrap().id();
        assert!(viewer.extra.coverflow.running.is_none());
        assert!(viewer.extra.coverflow.textures.is_empty());
        viewer.ensure_overview(&ctx);
        assert_eq!(viewer.overview.as_ref().unwrap().id(), texture);
    }

    #[test]
    fn sequence_follows_sorted_visible_batch_and_skips_containers() {
        let mut entries = vec![entry("folder"), entry("a.jpg"), entry("book.zip"), entry("b.jpg"), entry("c.jpg")];
        entries[0].directory = true;
        entries[2].archive = true;
        let mut state = State::default();
        state.ensure_sequence(&entries, &[4, 2, 1, 3, 0], &BTreeSet::from([0, 2, 3, 4]));
        assert_eq!(state.sequence, [4, 3]);
        assert_eq!(state.position(Some(3)), Some(1));
        state.invalidate_sequence();
        state.ensure_sequence(&entries, &[1, 3, 4], &BTreeSet::from([1]));
        assert_eq!(state.sequence, [1, 3, 4]);
        assert_eq!(state.position(Some(3)), Some(1));
    }

    #[test]
    fn navigation_and_preload_share_visible_order_and_respect_boundaries() {
        let mut folder = entry("folder");
        folder.directory = true;
        let mut viewer = super::super::browser_ui::tests::viewer(vec![
            folder, entry("a.jpg"), entry("b.jpg"), entry("c.jpg"), entry("d.jpg"),
        ]);
        viewer.extra.visible = vec![4, 2, 0, 1, 3];
        viewer.extra.selection = BTreeSet::from([0, 1, 2, 4]);
        viewer.selected = Some(2);
        assert_eq!(viewer.navigation_target(true, false), Some(1));
        assert_eq!(viewer.navigation_target(false, false), Some(4));
        assert_eq!(viewer.preload_neighbors(true), [Some(1), Some(4)]);
        assert_eq!(viewer.preload_neighbors(false), [Some(4), Some(1)]);
        assert_eq!(viewer.navigation_target(true, true), Some(1));
        assert_eq!(viewer.navigation_target(false, true), Some(4));

        viewer.selected = Some(1);
        assert_eq!(viewer.navigation_target(true, false), None);
        assert_eq!(viewer.preload_neighbors(true), [None, Some(2)]);
        viewer.extra.visible = vec![4, 3];
        viewer.extra.coverflow.invalidate_sequence();
        assert_eq!(viewer.navigation_target(false, false), Some(4));
        assert_eq!(viewer.preload_neighbors(true), [None, None]);
        assert_eq!(viewer.extra.coverflow.sequence_builds, 2);
    }

    #[test]
    fn stable_large_library_frames_do_not_rebuild_or_copy_the_sequence() {
        let entries = vec![entry("a.jpg"); 100_000];
        let visible: Vec<_> = (0..entries.len()).collect();
        let mut state = State::default();
        state.ensure_sequence(&entries, &visible, &BTreeSet::new());
        let pointer = state.sequence.as_ptr();
        for _ in 0..120 {
            state.ensure_sequence(&[], &[], &BTreeSet::new());
            assert_eq!(state.position(Some(99_999)), Some(99_999));
            assert_eq!(state.sequence.as_ptr(), pointer);
        }
        assert_eq!(state.sequence_builds, 1);
    }

    #[test]
    fn covers_shrink_fade_and_stay_separated_throughout_the_scroll() {
        let (center, alpha) = cover_style(0.0, 128.0);
        let (near, near_alpha) = cover_style(1.0, 128.0);
        let (far, far_alpha) = cover_style(5.0, 128.0);
        assert_eq!((center, alpha), (128.0, 1.0));
        assert!(center > near && near > far && alpha > near_alpha && near_alpha > far_alpha);
        assert!(near > center * 0.9 && far > center * 0.78);
        for phase in [0.0, 0.1, 0.5, 0.9, 1.0] {
            for position in -6..6 {
                let distance = position as f32 - phase;
                let right = cover_offset(distance, 128.0) + cover_style(distance, 128.0).0 / 2.0;
                let next_left = cover_offset(distance + 1.0, 128.0) - cover_style(distance + 1.0, 128.0).0 / 2.0;
                assert!(next_left - right >= 7.99);
            }
        }
    }

    #[test]
    fn rendered_photos_remain_axis_aligned_and_keep_their_aspect_ratio() {
        let mut viewer = super::super::browser_ui::tests::viewer(vec![entry("photo.jpg"); 5]);
        viewer.browser = false;
        viewer.selected = Some(2);
        let ctx = egui::Context::default();
        let texture = ctx.load_texture("flat-photo",
            egui::ColorImage::new([80, 40], Color32::WHITE), egui::TextureOptions::LINEAR);
        for index in 0..5 { viewer.extra.coverflow.insert(index, texture.clone()); }
        let output = ctx.run(egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))),
            ..Default::default()
        }, |ctx| viewer.draw_coverflow(ctx));
        let mut photos = 0;
        for clipped in output.shapes {
            let egui::Shape::Mesh(mesh) = clipped.shape else { continue; };
            if mesh.texture_id != texture.id() { continue; }
            photos += 1;
            assert_eq!(mesh.vertices.len(), 4);
            let bounds = mesh.calc_bounds();
            assert!((bounds.width() / bounds.height() - 2.0).abs() < 0.0001);
            for vertex in &mesh.vertices {
                assert!(vertex.pos.x == bounds.left() || vertex.pos.x == bounds.right());
                assert!(vertex.pos.y == bounds.top() || vertex.pos.y == bounds.bottom());
            }
            let cover = viewer.extra.coverflow.last_covers.iter()
                .find(|cover| cover.rect.center().distance(bounds.center()) < 0.001).unwrap();
            assert!(cover.rect.shrink(3.99).contains_rect(bounds));
        }
        assert_eq!(photos, 5);
    }

    #[test]
    fn bounded_lru_and_cancelled_source_results_do_not_reenter_the_gallery() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        for index in 0..MAX_COVERS {
            let texture = ctx.load_texture(format!("test-{index}"),
                egui::ColorImage::new([128, 128], Color32::WHITE), egui::TextureOptions::LINEAR);
            state.insert(index, texture);
        }
        state.texture(0);
        state.insert(32, state.textures[&1].clone());
        assert_eq!(state.textures.len(), MAX_COVERS);
        assert!(state.textures.contains_key(&0) && !state.textures.contains_key(&1));
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        state.running = Some(ThumbnailTask { epoch: state.epoch, index: 4, cancel: cancel.clone(), receiver });
        state.reset_source();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(state.running.is_some());
        sender.send(Ok(worker::Output { reply: worker::Reply { width: 1, height: 1, ..Default::default() },
            rgba: vec![255; 4], preview: Vec::new() })).unwrap();
        state.poll(&ctx);
        assert!(state.running.is_none() && state.textures.is_empty() && state.failed.is_empty());
    }

    #[test]
    fn placeholder_cover_click_selects_the_same_filtered_image_as_its_counter() {
        let dir = tempfile::tempdir().unwrap();
        let mut viewer = super::super::browser_ui::tests::viewer(vec![entry("a.jpg"), entry("b.jpg"), entry("c.jpg")]);
        viewer.browser = false;
        viewer.source = Some(dir.path().to_owned());
        let ctx = egui::Context::default();
        let input = |events| egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))), events, ..Default::default() };
        let _ = ctx.run(input(Vec::new()), |ctx| viewer.draw_coverflow(ctx));
        assert_eq!(viewer.extra.coverflow.position(Some(0)), Some(0));
        let point = viewer.extra.coverflow.last_covers.iter().find(|cover| cover.index == 1).unwrap().rect.center();
        for pressed in [true, false] {
            let _ = ctx.run(input(vec![egui::Event::PointerMoved(point), egui::Event::PointerButton {
                pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
            }]), |ctx| viewer.draw_coverflow(ctx));
        }
        assert_eq!(viewer.selected, Some(1));
        assert!(matches!(viewer.pending.as_ref().map(|t| &t.operation), Some(Operation::Load { index: 1, .. })));
    }

    #[test]
    fn missing_covers_wake_dispatch_without_mouse_input_and_ready_covers_go_idle() {
        let dir = tempfile::tempdir().unwrap();
        let mut viewer = super::super::browser_ui::tests::viewer(vec![entry("a.jpg"), entry("b.jpg")]);
        viewer.browser = false;
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))), ..Default::default() };
        let mut delay = std::time::Duration::ZERO;
        for _ in 0..8 {
            let output = ctx.run(input(), |ctx| viewer.draw_coverflow(ctx));
            delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        }
        assert_eq!(delay, std::time::Duration::MAX);
        viewer.source = Some(dir.path().to_owned());
        let output = ctx.run(input(), |ctx| viewer.draw_coverflow(ctx));
        assert_eq!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay, std::time::Duration::ZERO);
        for index in 0..2 {
            let texture = ctx.load_texture(format!("ready-{index}"),
                egui::ColorImage::new([1, 1], Color32::WHITE), egui::TextureOptions::LINEAR);
            viewer.extra.coverflow.insert(index, texture);
        }
        for _ in 0..8 {
            let output = ctx.run(input(), |ctx| viewer.draw_coverflow(ctx));
            delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        }
        assert_eq!(delay, std::time::Duration::MAX);
    }

    #[test]
    fn small_windows_reduce_visible_covers_and_preserve_the_main_canvas() {
        let mut viewer = super::super::browser_ui::tests::viewer(vec![entry("photo.jpg"); 100]);
        viewer.browser = false;
        viewer.selected = Some(50);
        for (width, height, covers) in [(1000.0, 700.0, 7), (360.0, 300.0, 5), (160.0, 240.0, 3)] {
            let ctx = egui::Context::default();
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, height))), ..Default::default() };
            let _ = ctx.run(input, |ctx| {
                viewer.draw_coverflow(ctx);
                assert_eq!(ctx.available_rect().width(), width);
                assert!(ctx.available_rect().height() >= 120.0);
            });
            assert_eq!(viewer.extra.coverflow.wanted.len(), covers);
            assert_eq!(viewer.extra.coverflow.position(Some(50)), Some(50));
        }
    }

    #[test]
    fn flat_cards_click_individually_and_the_gap_does_not_select() {
        let dir = tempfile::tempdir().unwrap();
        let mut viewer = super::super::browser_ui::tests::viewer(
            ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"].map(entry).to_vec());
        viewer.browser = false;
        viewer.selected = Some(2);
        viewer.source = Some(dir.path().to_owned());
        let ctx = egui::Context::default();
        let input = |events| egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))), events, ..Default::default() };
        let _ = ctx.run(input(Vec::new()), |ctx| viewer.draw_coverflow(ctx));
        let rect_for = |index| viewer.extra.coverflow.last_covers.iter().find(|cover| cover.index == index).unwrap().rect;
        let current = rect_for(2);
        let left = rect_for(1);
        let right = rect_for(3);
        assert!(left.right() < current.left() && current.right() < right.left());
        let gap = egui::pos2((left.right() + current.left()) / 2.0, current.center().y);
        let points = [current.center(), gap, left.center()];
        assert!(!current.contains(points[2]));
        for (attempt, point) in points.into_iter().enumerate() {
            for pressed in [true, false] {
                let _ = ctx.run(input(vec![egui::Event::PointerMoved(point), egui::Event::PointerButton {
                    pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
                }]), |ctx| viewer.draw_coverflow(ctx));
            }
            if attempt == 2 {
                assert_eq!(viewer.selected, Some(1));
                assert!(matches!(viewer.pending.as_ref().map(|t| &t.operation), Some(Operation::Load { index: 1, .. })));
            } else {
                assert_eq!(viewer.selected, Some(2));
                assert!(viewer.pending.is_none());
            }
        }
    }
}
