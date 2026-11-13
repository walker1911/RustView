use super::*;
use std::sync::Weak;

const BLUR: f32 = 7.0;
const GUTTER: f32 = BLUR * 3.0;

#[derive(PartialEq)]
struct View {
    bar: Rect,
    viewport: Rect,
    center: Pos2,
    scale: f32,
    rotation: u8,
    flip_x: bool,
    flip_y: bool,
    detail: bool,
    custom: bool,
}

struct Backdrop {
    image: Weak<Pixels>,
    view: View,
    texture: TextureHandle,
    uv: Rect,
}

#[derive(Default)]
pub(super) struct Glass {
    cached: Option<Backdrop>,
}

impl Glass {
    pub(super) fn paint(
        &mut self,
        painter: &egui::Painter,
        bar: Rect,
        image: &Arc<Pixels>,
        center: Pos2,
        scale: f32,
        rotation: u8,
        flip_x: bool,
        flip_y: bool,
        backdrop_is_custom: bool,
    ) {
        let viewport = painter.clip_rect();
        let visible = bar.intersect(viewport);
        if !visible.is_positive()
            || !visible.is_finite()
            || !scale.is_finite()
            || scale <= 0.0
            || painter.opacity() == 0.0
            || image.width == 0
            || image.height == 0
        {
            return;
        }
        let physical_scale = scale * painter.ctx().pixels_per_point();
        let view = View {
            bar,
            viewport,
            center,
            scale,
            rotation: rotation % 4,
            flip_x,
            flip_y,
            detail: image.width as f32 * physical_scale > image.preview_width as f32 * 1.05
                || image.height as f32 * physical_scale > image.preview_height as f32 * 1.05,
            custom: backdrop_is_custom,
        };
        let identity = Arc::downgrade(image);
        if !self
            .cached
            .as_ref()
            .is_some_and(|old| old.image.ptr_eq(&identity) && old.view == view)
        {
            let (pixels, uv) = render_backdrop(image, &view, visible);
            let texture = match self.cached.take() {
                Some(old) => {
                    let mut texture = old.texture;
                    texture.set(pixels, egui::TextureOptions::LINEAR);
                    texture
                }
                None => painter.ctx().load_texture(
                    "floating-toolbar-backdrop",
                    pixels,
                    egui::TextureOptions::LINEAR,
                ),
            };
            self.cached = Some(Backdrop {
                image: identity,
                view,
                texture,
                uv,
            });
        }
        if let Some(backdrop) = &self.cached {
            painter.add(
                egui::epaint::RectShape::filled(bar, 13.0, Color32::WHITE)
                    .with_texture(backdrop.texture.id(), backdrop.uv),
            );
        }
    }
}

fn render_backdrop(image: &Pixels, view: &View, visible: Rect) -> (egui::ColorImage, Rect) {
    // Sample a small rectangle in window coordinates, including the blur's gutter.
    // Even a large window or unusual layout cannot allocate a photo-sized texture.
    let area = visible.expand(GUTTER);
    let step = (area.width() / 384.0).max(area.height() / 96.0).max(2.0);
    let width = ((area.width() / step).ceil() as u32).clamp(1, 384);
    let height = ((area.height() / step).ceil() as u32).clamp(1, 96);
    let extent = egui::vec2(width as f32, height as f32) * step;
    let preview = (
        image.preview_width,
        image.preview_height,
        image.preview.as_slice(),
    );
    let full = (image.width, image.height, image.rgba.as_slice());
    let (width_px, height_px, rgba) = if !full.2.is_empty() && (view.detail || preview.2.is_empty()) {
        full
    } else {
        preview
    };
    let source = Source {
        rgba,
        width: width_px,
        height: height_px,
        dimensions: egui::vec2(image.width as f32, image.height as f32),
    };
    let patch = image::RgbaImage::from_fn(width, height, |x, y| {
        let pos = area.min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5) * step;
        image::Rgba(sample_backdrop(
            &source,
            view,
            pos.clamp(view.viewport.min, view.viewport.max),
        ))
    });
    let blurred = image::imageops::blur(&patch, BLUR / step);
    let uv = |pos: Pos2| ((pos - area.min) / extent).to_pos2();
    let pixels = egui::ColorImage::from_rgba_premultiplied(
        [width as usize, height as usize],
        blurred.as_raw(),
    );
    (
        pixels,
        Rect::from_min_max(uv(view.bar.min), uv(view.bar.max)),
    )
}

struct Source<'a> {
    rgba: &'a [u8],
    width: u32,
    height: u32,
    dimensions: Vec2,
}

fn sample_backdrop(source: &Source<'_>, view: &View, screen: Pos2) -> [u8; 4] {
    let mut point = rotate((screen - view.center) / view.scale, (4 - view.rotation) % 4);
    if view.flip_x {
        point.x = -point.x;
    }
    if view.flip_y {
        point.y = -point.y;
    }
    let uv = point / source.dimensions + Vec2::splat(0.5);
    if !(0.0..1.0).contains(&uv.x) || !(0.0..1.0).contains(&uv.y) {
        return if view.custom {
            [0; 4]
        } else {
            theme::CANVAS.to_array()
        };
    }

    let x = ((uv.x * source.width as f32) as usize).min(source.width.saturating_sub(1) as usize);
    let y = ((uv.y * source.height as f32) as usize).min(source.height.saturating_sub(1) as usize);
    let i = (y * source.width as usize + x) * 4;
    let pixel = source.rgba.get(i..i + 4).unwrap_or(&[0; 4]);
    // Match the canvas checker colors and their fixed 8-point window grid.
    let cell = (screen.x / 8.0).floor() as i32 + (screen.y / 8.0).floor() as i32;
    let checker = if cell % 2 == 0 { 224 } else { 176 };
    let alpha = u16::from(pixel[3]);
    let composite = |channel: usize| {
        ((u16::from(pixel[channel]) * alpha + checker * (255 - alpha) + 127) / 255) as u8
    };
    [composite(0), composite(1), composite(2), 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_follow_rotation_flip_and_zoom_with_correct_transparency() {
        let source = Source {
            rgba: &[255, 0, 0, 255, 0, 0, 255, 0],
            width: 2,
            height: 1,
            dimensions: egui::vec2(2.0, 1.0),
        };
        let mut view = View {
            bar: Rect::ZERO,
            viewport: Rect::EVERYTHING,
            center: Pos2::new(16.0, 20.0),
            scale: 4.0,
            rotation: 1,
            flip_x: true,
            flip_y: false,
            detail: true,
            custom: false,
        };
        assert_eq!(
            sample_backdrop(&source, &view, Pos2::new(16.0, 22.0)),
            [255, 0, 0, 255]
        );
        assert_eq!(
            sample_backdrop(&source, &view, Pos2::new(16.0, 18.0)),
            [224, 224, 224, 255]
        );
        let outside = Pos2::new(24.0, 20.0);
        assert_eq!(
            sample_backdrop(&source, &view, outside),
            theme::CANVAS.to_array()
        );
        view.custom = true;
        assert_eq!(sample_backdrop(&source, &view, outside), [0; 4]);
    }
}
