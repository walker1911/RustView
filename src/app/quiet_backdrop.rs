use super::{rotate, theme, Pixels};
use crate::worker::ContinuityPreview;
use egui::{Color32, Mesh, Painter, Rect, TextureHandle, TextureOptions, Vec2};
use std::sync::{Arc, Weak};

struct Backdrop {
    image: Weak<Pixels>,
    size: Vec2,
    texture: TextureHandle,
}

#[derive(Default)]
pub(super) struct State {
    cached: Option<Backdrop>,
}

impl State {
    pub(super) fn clear(&mut self) {
        self.cached = None;
    }

    pub(super) fn paint(
        &mut self,
        painter: &Painter,
        viewport: Rect,
        image: Option<&Arc<Pixels>>,
        rotation: u8,
        flip_x: bool,
        flip_y: bool,
    ) {
        if !viewport.is_positive() {
            return;
        }
        if let Some(image) = image {
            let identity = Arc::downgrade(image);
            if !self
                .cached
                .as_ref()
                .is_some_and(|old| old.image.ptr_eq(&identity))
            {
                let sample = ContinuityPreview::sample(
                    image.preview_width,
                    image.preview_height,
                    &image.preview,
                    image.width,
                    image.height,
                )
                .or_else(|| {
                    ContinuityPreview::sample(
                        image.width,
                        image.height,
                        &image.rgba,
                        image.width,
                        image.height,
                    )
                });
                let Some(sample) = sample else {
                    self.clear();
                    return;
                };
                let size = egui::vec2(sample.source_width as f32, sample.source_height as f32);
                let pixels = blur(sample);
                let texture = if let Some(old) = &mut self.cached {
                    old.texture.set(pixels, TextureOptions::LINEAR);
                    old.texture.clone()
                } else {
                    painter
                        .ctx()
                        .load_texture("photo-soft-focus", pixels, TextureOptions::LINEAR)
                };
                self.cached = Some(Backdrop {
                    image: identity,
                    size,
                    texture,
                });
            }
        }
        let Some(backdrop) = &self.cached else {
            return;
        };
        let rotated_size = if rotation % 2 == 0 {
            backdrop.size
        } else {
            backdrop.size.yx()
        };
        let scale = (viewport.size() / rotated_size).max_elem() * 1.12;
        let flip = egui::vec2(
            if flip_x { -1.0 } else { 1.0 },
            if flip_y { -1.0 } else { 1.0 },
        );
        let mut mesh = Mesh::with_texture(backdrop.texture.id());
        for pos in [
            viewport.left_top(),
            viewport.right_top(),
            viewport.right_bottom(),
            viewport.left_bottom(),
        ] {
            let uv = rotate((pos - viewport.center()) / scale, (4 - rotation % 4) % 4) * flip
                / backdrop.size
                + Vec2::splat(0.5);
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: uv.to_pos2(),
                color: Color32::WHITE,
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(mesh);
    }
}

fn blur(sample: ContinuityPreview) -> egui::ColorImage {
    let ContinuityPreview {
        width,
        height,
        mut rgba,
        ..
    } = sample;
    let graphite = theme::CANVAS.to_array();
    for pixel in rgba.chunks_exact_mut(4) {
        let lightness =
            0.21 * f32::from(pixel[0]) + 0.72 * f32::from(pixel[1]) + 0.07 * f32::from(pixel[2]);
        let opacity = f32::from(pixel[3]) * (0.74 / 255.0);
        for (channel, base) in pixel[..3].iter_mut().zip(graphite) {
            let muted = 0.54 * f32::from(*channel) + 0.46 * lightness;
            *channel = (f32::from(base) * (1.0 - opacity) + muted * 0.29 * opacity).round() as u8;
        }
        pixel[3] = 255;
    }
    // The existing continuity sampler caps this at 64×64 before filtering.
    let patch = image::RgbaImage::from_raw(width, height, rgba).expect("valid continuity sample");
    let blurred = image::imageops::blur(&patch, (width.max(height) as f32 / 8.0).max(0.5));
    egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], blurred.as_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_is_bounded_and_transparent_colors_leave_no_tint() {
        let rgba = [250, 0, 255, 0].repeat(260 * 400);
        let sample = ContinuityPreview::sample(260, 400, &rgba, 260, 400).unwrap();
        let image = blur(sample);
        assert_eq!(image.size, [41, 64]);
        assert!(image.pixels.iter().all(|color| *color == theme::CANVAS));
    }
}
