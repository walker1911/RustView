use super::{rotate, Pixels};
use egui::{Color32, Context, Mesh, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use std::{sync::Arc, time::{Duration, Instant}};

const PREVIEW_SIDE: u32 = 64;

#[derive(Clone)]
pub(super) struct PreviewPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    pub source_width: u32,
    pub source_height: u32,
}

impl PreviewPixels {
    pub fn sample(width: u32, height: u32, rgba: &[u8], source_width: u32, source_height: u32) -> Option<Self> {
        let length = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        if width == 0 || height == 0 || rgba.len() != length { return None; }
        let largest = width.max(height);
        let (small_width, small_height) = if largest > PREVIEW_SIDE {
            (((width as u64 * PREVIEW_SIDE as u64 / largest as u64) as u32).max(1),
             ((height as u64 * PREVIEW_SIDE as u64 / largest as u64) as u32).max(1))
        } else { (width, height) };
        let mut pixels = Vec::with_capacity(small_width as usize * small_height as usize * 4);
        for y in 0..small_height {
            let source_y = ((2 * y as u64 + 1) * height as u64 / (2 * small_height as u64)) as usize;
            for x in 0..small_width {
                let source_x = ((2 * x as u64 + 1) * width as u64 / (2 * small_width as u64)) as usize;
                let offset = (source_y * width as usize + source_x) * 4;
                pixels.extend_from_slice(&rgba[offset..offset + 4]);
            }
        }
        Some(Self {
            width: small_width, height: small_height, rgba: pixels.into(),
            source_width: if source_width > 0 { source_width } else { width },
            source_height: if source_height > 0 { source_height } else { height },
        })
    }

    fn upload(&self, ctx: &Context) -> TextureHandle {
        ctx.load_texture("continuity-preview", egui::ColorImage::from_rgba_unmultiplied(
            [self.width as usize, self.height as usize], &self.rgba), TextureOptions::NEAREST)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placement {
    pub scale: f32,
    pub pan: Vec2,
    pub rotation: u8,
    pub flip_x: bool,
    pub flip_y: bool,
    pub fit: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self { scale: 1.0, pan: Vec2::ZERO, rotation: 0, flip_x: false, flip_y: false, fit: true }
    }
}

struct Frame {
    epoch: u64,
    index: usize,
    name: String,
    pixels: PreviewPixels,
    texture: Option<TextureHandle>,
    placement: Placement,
}

struct Target {
    index: usize,
    name: String,
}

#[derive(Clone, Copy, PartialEq)]
struct Recorded {
    epoch: u64,
    index: usize,
    token: usize,
    placement: Placement,
}

#[derive(Default)]
pub(super) struct State {
    epoch: u64,
    frame: Option<Frame>,
    target: Option<Target>,
    waiting: bool,
    waiting_since: Option<Instant>,
    recorded: Option<Recorded>,
}

impl State {
    pub fn begin_source(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.target = None;
        self.waiting = false;
        self.waiting_since = None;
        self.recorded = None;
    }

    pub fn begin_target(&mut self, index: usize, name: String) {
        self.target = Some(Target { index, name });
        self.waiting = true;
        self.waiting_since = Some(Instant::now());
    }

    pub fn ready(&mut self, index: usize) {
        if self.target.as_ref().is_some_and(|target| target.index == index) {
            self.waiting = false;
            self.waiting_since = None;
        }
    }

    pub fn stage_preview(&mut self, index: usize, pixels: PreviewPixels) {
        if !self.waiting || !self.target.as_ref().is_some_and(|target| target.index == index) { return; }
        if self.frame.as_ref().is_some_and(|frame| frame.epoch == self.epoch && frame.index == index) { return; }
        if pixels.width == 0 || pixels.height == 0 || pixels.width > PREVIEW_SIDE || pixels.height > PREVIEW_SIDE
            || pixels.source_width == 0 || pixels.source_height == 0
            || pixels.rgba.len() != pixels.width as usize * pixels.height as usize * 4 { return; }
        let name = self.target.as_ref().unwrap().name.clone();
        self.frame = Some(Frame { epoch: self.epoch, index, name, pixels,
            texture: None, placement: Placement::default() });
        self.recorded = None;
    }

    pub fn has_frame(&self) -> bool { self.frame.is_some() }
    pub fn is_waiting(&self) -> bool { self.waiting }
    pub fn waiting_elapsed(&self) -> Duration { self.waiting_since.map(|time| time.elapsed()).unwrap_or_default() }
    pub fn target_index(&self) -> Option<usize> { self.target.as_ref().map(|target| target.index) }

    pub fn clear(&mut self) {
        self.begin_source();
        self.frame = None;
    }

    pub fn texture_for(&self, index: usize) -> Option<TextureHandle> {
        self.frame.as_ref().filter(|frame| frame.epoch == self.epoch && frame.index == index)?.texture.clone()
    }

    pub fn capture_displayed(&mut self, ctx: &Context, index: usize, image: &Pixels,
        token: usize, placement: Placement, name: &str,
    ) -> Option<(PreviewPixels, TextureHandle)> {
        let record = Recorded { epoch: self.epoch, index, token, placement };
        if self.recorded == Some(record) && self.frame.as_ref().is_some_and(|frame|
            frame.epoch == self.epoch && frame.index == index && frame.name == name)
        { return None; }
        let same_pixels = self.recorded.is_some_and(|old| old.epoch == record.epoch
            && old.index == record.index && old.token == record.token)
            && self.frame.as_ref().is_some_and(|frame| frame.epoch == self.epoch && frame.index == index);
        if same_pixels {
            let frame = self.frame.as_mut().unwrap();
            frame.placement = placement;
            if frame.name != name { frame.name = name.to_owned(); }
            self.recorded = Some(record);
            return None;
        }
        let pixels = PreviewPixels::sample(image.preview_width, image.preview_height, &image.preview,
            image.width, image.height).or_else(|| PreviewPixels::sample(image.width, image.height,
                &image.rgba, image.width, image.height))?;
        let texture = pixels.upload(ctx);
        self.frame = Some(Frame { epoch: self.epoch, index, name: name.to_owned(), pixels: pixels.clone(),
            texture: Some(texture.clone()), placement });
        self.recorded = Some(record);
        if !self.waiting || self.target.is_none() {
            self.target = Some(Target { index, name: name.to_owned() });
        }
        Some((pixels, texture))
    }

    pub fn draw(&mut self, painter: &Painter, viewport: Rect, ctx: &Context) -> bool {
        let Some(frame) = &mut self.frame else { return false; };
        let texture = frame.texture.get_or_insert_with(|| frame.pixels.upload(ctx));
        let corners = frame_corners(viewport, &frame.pixels, frame.placement, ctx.pixels_per_point());
        let mut mesh = Mesh::with_texture(texture.id());
        for (pos, uv) in corners.into_iter().zip([
            Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0),
        ]) {
            mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        let painter = painter.with_clip_rect(viewport);
        painter.add(mesh);
        if self.waiting {
            if let Some(target) = &self.target {
                let text = if frame.epoch == self.epoch && frame.index == target.index {
                    format!("目标：{} · 低清预览", target.name)
                } else {
                    format!("目标：{} · 暂显示 {}", target.name, frame.name)
                };
                painter.text(viewport.left_bottom() + Vec2::new(14.0, -14.0),
                    egui::Align2::LEFT_BOTTOM, text, egui::FontId::proportional(14.0), Color32::WHITE);
            }
        }
        true
    }
}

fn frame_corners(viewport: Rect, pixels: &PreviewPixels, placement: Placement, pixels_per_point: f32) -> [Pos2; 4] {
    let width = pixels.source_width as f32;
    let height = pixels.source_height as f32;
    let dimensions = if placement.rotation % 2 == 0 { Vec2::new(width, height) } else { Vec2::new(height, width) };
    let scale = if placement.fit {
        (viewport.width() / dimensions.x).min(viewport.height() / dimensions.y).min(1.0 / pixels_per_point)
    } else { placement.scale };
    [Vec2::new(-width, -height), Vec2::new(width, -height), Vec2::new(width, height), Vec2::new(-width, height)]
        .map(|point| {
            let mut point = point * 0.5;
            if placement.flip_x { point.x = -point.x; }
            if placement.flip_y { point.y = -point.y; }
            viewport.center() + placement.pan + rotate(point, placement.rotation) * scale
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(color: [u8; 4]) -> Pixels {
        Pixels { width: 512, height: 256, rgba: color.repeat(512 * 256),
            preview: color.repeat(128 * 64), preview_width: 128, preview_height: 64,
            note: String::new(), record: None }
    }

    fn capture(state: &mut State, ctx: &Context, index: usize) -> TextureHandle {
        state.begin_target(index, format!("{index}.png"));
        state.ready(index);
        state.capture_displayed(ctx, index, &pixels([255, 20, 40, 255]), index,
            Placement::default(), &format!("{index}.png")).unwrap().1
    }

    #[test]
    fn ready_without_paint_then_another_target_keeps_the_last_displayed_frame() {
        let ctx = Context::default();
        let mut state = State::default();
        let a = capture(&mut state, &ctx, 0).id();
        state.begin_target(1, "B.png".into());
        state.ready(1);
        state.begin_target(2, "C.png".into());
        assert!(state.has_frame() && state.is_waiting());
        assert_eq!(state.target_index(), Some(2));
        assert_eq!(state.texture_for(0).unwrap().id(), a);
        assert!(state.texture_for(1).is_none() && state.texture_for(2).is_none());
    }

    #[test]
    fn cancelled_reversed_failed_and_password_waiting_targets_never_clear_the_frame() {
        let ctx = Context::default();
        let mut state = State::default();
        let a = capture(&mut state, &ctx, 0).id();
        for index in [1, 2, 1, 0, 3] {
            state.begin_target(index, format!("target-{index}.png"));
            // Failure or password requests produce no ready/capture callback.
            assert!(state.has_frame() && state.is_waiting());
            assert_eq!(state.texture_for(0).unwrap().id(), a);
        }
        state.clear();
        assert!(!state.has_frame() && !state.is_waiting() && state.target_index().is_none());
        assert_eq!(state.waiting_elapsed(), Duration::ZERO);
    }

    #[test]
    fn new_source_keeps_pixels_but_does_not_match_the_previous_sources_same_index() {
        let ctx = Context::default();
        let mut state = State::default();
        let _ = capture(&mut state, &ctx, 0);
        state.begin_source();
        assert!(state.has_frame());
        assert!(state.texture_for(0).is_none());
        state.begin_target(0, "new-folder.png".into());
        state.stage_preview(0, PreviewPixels::sample(1, 1, &[20, 30, 40, 255], 6000, 4000).unwrap());
        assert_eq!(state.frame.as_ref().unwrap().pixels.source_width, 6000);
    }

    #[test]
    fn waiting_age_resets_only_for_the_active_target_or_source() {
        let mut state = State::default();
        state.begin_target(2, "target.png".into());
        state.waiting_since = Some(Instant::now() - Duration::from_millis(300));
        state.ready(1);
        assert!(state.is_waiting() && state.waiting_elapsed() >= Duration::from_millis(300));
        state.ready(2);
        assert!(!state.is_waiting() && state.waiting_elapsed() == Duration::ZERO);
        state.begin_target(3, "next.png".into());
        state.begin_source();
        assert!(!state.is_waiting() && state.waiting_elapsed() == Duration::ZERO);
    }

    #[test]
    fn previews_are_bounded_and_static_or_transformed_frames_reuse_the_same_upload() {
        let ctx = Context::default();
        let mut state = State::default();
        let image = Arc::new(pixels([20, 40, 60, 128]));
        let (tiny, texture) = state.capture_displayed(&ctx, 0, &image, 99,
            Placement::default(), "A.png").unwrap();
        assert_eq!((tiny.width, tiny.height), (64, 32));
        assert!(tiny.rgba.len() <= 64 * 64 * 4);
        assert_eq!(Arc::strong_count(&image), 1);
        let cloned = tiny.clone();
        assert!(Arc::ptr_eq(&tiny.rgba, &cloned.rgba));
        for step in 0..120 {
            let placement = Placement { pan: Vec2::new(step as f32, 4.0), fit: false, ..Default::default() };
            assert!(state.capture_displayed(&ctx, 0, &image, 99, placement, "A.png").is_none());
            assert_eq!(state.texture_for(0).unwrap().id(), texture.id());
            assert!(Arc::ptr_eq(&tiny.rgba, &state.frame.as_ref().unwrap().pixels.rgba));
        }
    }

    #[test]
    fn preview_staging_is_target_bound_and_transparent_replacement_draws_one_image() {
        let ctx = Context::default();
        let mut state = State::default();
        let _ = capture(&mut state, &ctx, 0);
        state.begin_target(1, "transparent.png".into());
        let tiny = PreviewPixels::sample(2, 1, &[255, 0, 0, 0, 0, 255, 0, 255], 6000, 4000).unwrap();
        state.stage_preview(2, tiny.clone());
        assert_eq!(state.frame.as_ref().unwrap().index, 0);
        state.stage_preview(1, tiny.clone());
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0));
        let output = ctx.run(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            assert!(state.draw(&painter, viewport, ctx));
        });
        let texture = state.texture_for(1).unwrap().id();
        state.stage_preview(1, tiny);
        assert_eq!(state.texture_for(1).unwrap().id(), texture);
        assert_eq!(state.frame.as_ref().unwrap().pixels.rgba[3], 0);
        let meshes: Vec<_> = output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) => Some(mesh), _ => None,
        }).collect();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].texture_id, texture);
        let quad = frame_corners(viewport, &state.frame.as_ref().unwrap().pixels, Placement::default(), 1.0);
        assert!((quad[1].x - quad[0].x - 900.0).abs() < 0.001);
        assert!((quad[3].y - quad[0].y - 600.0).abs() < 0.001);
    }
}
