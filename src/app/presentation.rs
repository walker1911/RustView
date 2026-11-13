use super::{rotate, theme, Pixels};
use crate::worker::{ContinuityPreview, CONTINUITY_SIDE as PREVIEW_SIDE};
use egui::{Color32, Context, Mesh, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use std::{sync::Arc, time::{Duration, Instant}};

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
        ContinuityPreview::sample(width, height, rgba, source_width, source_height).map(Self::from)
    }

    fn upload(&self, ctx: &Context) -> TextureHandle {
        ctx.load_texture("continuity-preview", egui::ColorImage::from_rgba_unmultiplied(
            [self.width as usize, self.height as usize], &self.rgba), TextureOptions::NEAREST)
    }
}

impl From<ContinuityPreview> for PreviewPixels {
    fn from(preview: ContinuityPreview) -> Self {
        Self { width: preview.width, height: preview.height, rgba: preview.rgba.into(),
            source_width: preview.source_width, source_height: preview.source_height }
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
    token: usize,
    dimensions: [u32; 2],
    overview: TextureHandle,
    placement: Placement,
}

struct Target {
    index: usize,
    name: String,
    started: Instant,
    preview: Option<TargetPreview>,
}

struct TargetPreview {
    pixels: PreviewPixels,
    texture: Option<TextureHandle>,
}

#[derive(Default)]
pub(super) struct State {
    epoch: u64,
    frame: Option<Frame>,
    target: Option<Target>,
}

impl State {
    pub fn begin_source(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.cancel_target();
    }

    pub fn cancel_target(&mut self) {
        self.target = None;
    }

    pub fn begin_target(&mut self, index: usize, name: String) {
        self.target = Some(Target { index, name, started: Instant::now(), preview: None });
    }

    pub fn ready(&mut self, index: usize) {
        if self.target.as_ref().is_some_and(|target| target.index == index) {
            self.cancel_target();
        }
    }

    pub fn stage_preview(&mut self, index: usize, pixels: PreviewPixels) {
        let Some(target) = self.target.as_mut().filter(|target| target.index == index) else { return; };
        if target.preview.is_some() { return; }
        if self.frame.as_ref().is_some_and(|frame| {
            frame.epoch == self.epoch && frame.index == index && frame.name == target.name
        }) { return; }
        if pixels.width == 0 || pixels.height == 0 || pixels.width > PREVIEW_SIDE || pixels.height > PREVIEW_SIDE
            || pixels.source_width == 0 || pixels.source_height == 0
            || pixels.rgba.len() != pixels.width as usize * pixels.height as usize * 4 { return; }
        target.preview = Some(TargetPreview { pixels, texture: None });
    }

    pub fn has_frame(&self) -> bool {
        self.frame.is_some() || self.target.as_ref().is_some_and(|target| target.preview.is_some())
    }

    pub(super) fn frame_bounds(&self, viewport: Rect, pixels_per_point: f32) -> Rect {
        let frame = self.target.as_ref().and_then(|target| target.preview.as_ref())
            .map(|preview| ([preview.pixels.source_width, preview.pixels.source_height], Placement::default()))
            .or_else(|| self.frame.as_ref().map(|frame| (frame.dimensions, frame.placement)));
        frame.map_or(Rect::NOTHING, |(dimensions, placement)| {
            Rect::from_points(&frame_corners(viewport, dimensions, placement, pixels_per_point)).intersect(viewport)
        })
    }
    pub fn is_waiting(&self) -> bool { self.target.is_some() }
    pub fn has_target_preview(&self, index: usize) -> bool {
        self.target.as_ref().is_some_and(|target| target.index == index && target.preview.is_some())
    }
    pub fn waiting_elapsed(&self) -> Duration {
        self.target.as_ref().map(|target| target.started.elapsed()).unwrap_or_default()
    }
    pub fn target_index(&self) -> Option<usize> { self.target.as_ref().map(|target| target.index) }

    pub fn clear(&mut self) {
        self.begin_source();
        self.frame = None;
    }

    pub fn texture_for(&self, index: usize) -> Option<TextureHandle> {
        self.target.as_ref().filter(|target| target.index == index)
            .and_then(|target| target.preview.as_ref())
            .and_then(|preview| preview.texture.clone())
            .or_else(|| self.frame.as_ref().filter(|frame| frame.epoch == self.epoch && frame.index == index)
                .map(|frame| frame.overview.clone()))
    }

    pub fn capture_displayed(&mut self, ctx: &Context, index: usize, image: &Arc<Pixels>,
        overview: &TextureHandle, placement: Placement, name: &str,
    ) -> Option<(PreviewPixels, TextureHandle)> {
        let token = Arc::as_ptr(image) as usize;
        if let Some(frame) = self.frame.as_mut().filter(|frame|
            frame.epoch == self.epoch && frame.index == index && frame.token == token
                && frame.overview.id() == overview.id()
        ) {
            frame.placement = placement;
            if frame.name != name { frame.name = name.to_owned(); }
            return None;
        }
        // Reuse the overview that actually drew. The 64px texture is only for
        // an identified target preview and the filmstrip.
        self.frame = Some(Frame { epoch: self.epoch, index, name: name.to_owned(), token,
            dimensions: [image.width, image.height], overview: overview.clone(), placement });
        let pixels = PreviewPixels::sample(image.preview_width, image.preview_height, &image.preview,
            image.width, image.height).or_else(|| PreviewPixels::sample(image.width, image.height,
                &image.rgba, image.width, image.height))?;
        let texture = pixels.upload(ctx);
        Some((pixels, texture))
    }

    pub fn draw(&mut self, painter: &Painter, viewport: Rect, ctx: &Context, show_label: bool) -> bool {
        let target = self.target.as_mut();
        let preview = target.and_then(|target| {
            target.preview.as_mut().map(|preview| (&target.name, preview))
        });
        let (texture, dimensions, placement, label) = if let Some((name, preview)) = preview {
            let texture = preview.texture.get_or_insert_with(|| preview.pixels.upload(ctx)).id();
            (texture, [preview.pixels.source_width, preview.pixels.source_height],
                Placement::default(), Some(format!("目标：{name} · 低清预览")))
        } else if let Some(frame) = &self.frame {
            let label = self.target.as_ref().map(|target| {
                format!("目标：{} · 暂显示 {}", target.name, frame.name)
            });
            (frame.overview.id(), frame.dimensions, frame.placement, label)
        } else {
            return false;
        };
        let corners = frame_corners(viewport, dimensions, placement, ctx.pixels_per_point());
        let mut mesh = Mesh::with_texture(texture);
        for (pos, uv) in corners.into_iter().zip([
            Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0),
        ]) {
            mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        let painter = painter.with_clip_rect(viewport);
        super::paint_transparency(&painter, Rect::from_points(&corners));
        painter.add(mesh);
        if let Some(label) = label.filter(|_| show_label) {
            let galley = painter.layout_no_wrap(
                label,
                egui::TextStyle::Small.resolve(&ctx.style()),
                theme::TEXT,
            );
            let position = viewport.left_bottom() + Vec2::new(14.0, -14.0 - galley.size().y);
            painter.rect_filled(
                Rect::from_min_size(position, galley.size()).expand2(Vec2::new(8.0, 4.0)),
                5.0,
                theme::CHROME,
            );
            painter.galley(position, galley, theme::TEXT);
        }
        true
    }
}

fn frame_corners(viewport: Rect, dimensions: [u32; 2], placement: Placement, pixels_per_point: f32) -> [Pos2; 4] {
    let [width, height] = dimensions.map(|size| size as f32);
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

    fn image_and_overview(ctx: &Context) -> (Arc<Pixels>, TextureHandle) {
        let color = [255, 20, 40, 255];
        let image = Arc::new(Pixels { width: 512, height: 256, rgba: color.repeat(512 * 256),
            preview: color.repeat(256 * 128), preview_width: 256, preview_height: 128,
            note: String::new(), record: None });
        let overview = ctx.load_texture("overview", egui::ColorImage::from_rgba_unmultiplied(
            [256, 128], &image.preview), TextureOptions::LINEAR);
        (image, overview)
    }

    fn capture(state: &mut State, ctx: &Context, index: usize) -> TextureHandle {
        state.begin_target(index, format!("{index}.png"));
        state.ready(index);
        let (image, overview) = image_and_overview(ctx);
        let _ = state.capture_displayed(ctx, index, &image, &overview,
            Placement::default(), &format!("{index}.png")).unwrap();
        overview
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
        assert_eq!(state.target.as_ref().unwrap().preview.as_ref().unwrap().pixels.source_width, 6000);
        assert_eq!(state.frame.as_ref().unwrap().dimensions, [512, 256]);
    }

    #[test]
    fn waiting_age_resets_only_for_the_active_target_or_source() {
        let mut state = State::default();
        state.begin_target(2, "target.png".into());
        state.target.as_mut().unwrap().started = Instant::now() - Duration::from_millis(300);
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
        let (image, overview) = image_and_overview(&ctx);
        let (tiny, texture) = state.capture_displayed(&ctx, 0, &image, &overview,
            Placement::default(), "A.png").unwrap();
        assert_eq!((tiny.width, tiny.height), (64, 32));
        assert!(tiny.rgba.len() <= 64 * 64 * 4);
        assert_eq!(Arc::strong_count(&image), 1);
        assert_ne!(texture.id(), overview.id());
        for step in 0..120 {
            let placement = Placement { pan: Vec2::new(step as f32, 4.0), fit: false, ..Default::default() };
            assert!(state.capture_displayed(&ctx, 0, &image, &overview, placement, "A.png").is_none());
            assert_eq!(state.texture_for(0).unwrap().id(), overview.id());
            assert_eq!(state.frame.as_ref().unwrap().placement, placement);
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
            assert!(state.draw(&painter, viewport, ctx, true));
        });
        let texture = state.texture_for(1).unwrap().id();
        state.stage_preview(1, tiny);
        assert_eq!(state.texture_for(1).unwrap().id(), texture);
        let preview = state.target.as_ref().unwrap().preview.as_ref().unwrap();
        assert_eq!(preview.pixels.rgba[3], 0);
        let meshes: Vec<_> = output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) => Some(mesh), _ => None,
        }).collect();
        let checker = ctx.data(|data| data.get_temp::<TextureHandle>(egui::Id::new("transparency-checkerboard"))).unwrap();
        assert_eq!(meshes.iter().map(|mesh| mesh.texture_id).collect::<Vec<_>>(),
            [checker.id(), texture]);
        assert_eq!(meshes[0].calc_bounds(), meshes[1].calc_bounds());
        let quad = frame_corners(viewport, [preview.pixels.source_width, preview.pixels.source_height], Placement::default(), 1.0);
        assert!((quad[1].x - quad[0].x - 900.0).abs() < 0.001);
        assert!((quad[3].y - quad[0].y - 600.0).abs() < 0.001);
    }

    #[test]
    fn retarget_and_cancel_keep_the_last_clear_texture_and_discard_stale_previews() {
        let ctx = Context::default();
        let mut state = State::default();
        let previous = capture(&mut state, &ctx, 0).id();
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
        let draw = |state: &mut State| {
            let output = ctx.run(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ctx| {
                assert!(state.draw(&ctx.layer_painter(egui::LayerId::background()), viewport, ctx, true));
            });
            let mesh = output.shapes.iter().rev().find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => Some(mesh), _ => None,
            }).unwrap();
            (mesh.texture_id, output)
        };
        let green = PreviewPixels::sample(1, 1, &[0, 255, 0, 255], 512, 256).unwrap();
        let blue = PreviewPixels::sample(1, 1, &[0, 0, 255, 255], 512, 256).unwrap();

        state.begin_target(1, "green.png".into());
        assert_eq!(draw(&mut state).0, previous);
        state.stage_preview(1, green.clone());
        let green_texture = draw(&mut state).0;
        assert_ne!(green_texture, previous);

        state.begin_target(2, "blue.png".into());
        state.stage_preview(1, green.clone()); // The superseded worker returned late.
        assert_eq!(draw(&mut state).0, previous);
        assert!(state.texture_for(1).is_none());
        state.stage_preview(2, blue.clone());
        let (blue_texture, output) = draw(&mut state);
        assert_ne!(blue_texture, green_texture);
        let upload = &output.textures_delta.set.iter()
            .find(|(id, _)| *id == blue_texture).unwrap().1.image;
        let egui::ImageData::Color(upload) = upload else { panic!("expected target pixels") };
        assert_eq!(upload.pixels, [Color32::BLUE]);

        state.cancel_target();
        state.stage_preview(2, blue);
        assert_eq!(draw(&mut state).0, previous);
        assert!(state.texture_for(2).is_none());
        state.begin_source();
        state.begin_target(0, "different.png".into());
        assert!(state.texture_for(0).is_none());
        assert_eq!(draw(&mut state).0, previous);
        state.stage_preview(0, green);
        assert_ne!(draw(&mut state).0, previous);
    }
}
