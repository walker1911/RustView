use super::{paint_quad, Operation, Pixels, Request, Viewer};
use crate::{svg, worker};
use egui::{Painter, Pos2, Rect, TextureHandle};
use std::time::{Duration, Instant};

const SETTLE: Duration = Duration::from_millis(140);
const STEP: u32 = 128;

#[derive(Default)]
pub(super) struct View {
    wanted: u32,
    scale: f32,
    since: Option<Instant>,
    attempted: u32,
    detail: Option<TextureHandle>,
}

impl View {
    fn observe(&mut self, side: u32, scale: f32, now: Instant) -> bool {
        if self.wanted != side || self.scale != scale {
            self.wanted = side;
            self.scale = scale;
            self.since = Some(now);
            return false;
        }
        self.since
            .is_some_and(|since| now.duration_since(since) >= SETTLE)
    }
}

fn wanted_side(image: &Pixels, scale: f32, texture_limit: u32) -> u32 {
    let largest = f64::from(image.width.max(image.height));
    let pixels = f64::from(image.width) * f64::from(image.height);
    let pixel_limit = (svg::MAX_RASTER_PIXELS as f64 / pixels).sqrt() * largest;
    let limit = (pixel_limit.floor() as u32)
        .min(texture_limit)
        .min(svg::MAX_RASTER_EDGE);
    let desired = (largest * f64::from(scale)).ceil() as u32;
    (desired.min(limit).div_ceil(STEP) * STEP).min(limit)
}

impl Viewer {
    pub(super) fn image_fit_limit(&self, image: &Pixels) -> f32 {
        if image.svg_size.is_some() {
            f32::INFINITY
        } else if self.embedded_psd_preview {
            3.0
        } else {
            1.0
        }
    }

    pub(super) fn draw_svg(&mut self, painter: &Painter, image: &Pixels, center: Pos2, uv: Rect) {
        let ctx = painter.ctx();
        let rect = Rect::from_min_max(
            Pos2::ZERO,
            egui::pos2(image.width as f32, image.height as f32),
        );
        let sharp = self.svg.detail.as_ref();
        if let Some(texture) = sharp.or(self.overview.as_ref()) {
            paint_quad(
                painter,
                texture.id(),
                rect,
                uv,
                image,
                center,
                self.scale,
                self.rotation,
                self.flip_x,
                self.flip_y,
            );
        }
        let Some(index) = self
            .selected
            .filter(|&index| self.current_key.map(|key| key.0) == Some(index))
        else {
            return;
        };
        if self.browser || self.closing || self.extra.edited || self.extra.edit_dialog.is_some() {
            return;
        }

        let scale = self.target_scale * ctx.pixels_per_point();
        let side = wanted_side(image, scale, self.extra.max_texture);
        let now = Instant::now();
        let settled = self.svg.observe(side, scale, now);
        let source_side = image.width.max(image.height);
        if side <= self.svg.attempted.max(source_side + STEP / 2) {
            return;
        }
        if !settled {
            ctx.request_repaint_after(SETTLE);
            return;
        }
        if self.pending.is_some()
            || self.running.is_some()
            || self.extra.writing.is_some()
            || self.extra.pending_write.is_some()
            || self.password_dialog
            || self.deferred_image.is_some()
        {
            return;
        }
        let Some(source) = self.make_load(index, false) else {
            return;
        };
        self.svg.attempted = side;
        self.queue(
            Request::Svg {
                source: Box::new(source),
                width: side,
                height: side,
            },
            Operation::SvgRefine { index, side },
        );
        self.start_pending_task(ctx);
        crate::trace_startup("svg_refine_started");
    }

    pub(super) fn accept_svg_refinement(
        &mut self,
        index: usize,
        side: u32,
        output: worker::Output,
        ctx: &egui::Context,
    ) {
        let dimensions = [output.reply.width, output.reply.height];
        let Some(image) = self
            .current
            .as_ref()
            .filter(|image| image.svg_size.is_some())
        else {
            return;
        };
        if self.selected != Some(index)
            || self.current_key.map(|key| key.0) != Some(index)
            || image.svg_size != output.reply.svg_size
            || dimensions.contains(&0)
        {
            return;
        }
        if side != self.svg.wanted {
            self.svg.attempted = self
                .svg
                .detail
                .as_ref()
                .map_or(0, |texture| texture.size()[0].max(texture.size()[1]) as u32);
            return;
        }
        self.svg.detail = Some(ctx.load_texture(
            "svg-detail",
            egui::ColorImage::from_rgba_unmultiplied(dimensions.map(|n| n as usize), &output.rgba),
            egui::TextureOptions::LINEAR,
        ));
        crate::trace_startup("svg_refine_applied");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_zoom_has_to_settle_before_rendering() {
        let mut view = View::default();
        let start = Instant::now();
        assert!(!view.observe(2048, 1.0, start));
        assert!(!view.observe(2176, 1.1, start + SETTLE));
        let same_bucket = start + SETTLE + Duration::from_millis(1);
        assert!(!view.observe(2176, 1.12, same_bucket));
        assert!(view.observe(2176, 1.12, same_bucket + SETTLE));
    }
}
