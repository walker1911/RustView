use crate::ocr::TextLine;
use egui::{Color32, Context, Pos2, Rect, Response, Vec2};
use std::ops::Range;

struct Glyph {
    text: String,
    bounds: Rect,
}

struct Line {
    bounds: Rect,
    range: Range<usize>,
}

pub(super) struct Layout {
    glyphs: Vec<Glyph>,
    lines: Vec<Line>,
    anchor: usize,
    end: usize,
    dragging: bool,
    hovered: bool,
}

impl Layout {
    pub(super) fn new(lines: Vec<TextLine>, width: u32, height: u32, source: Vec2) -> Self {
        let scale = source / egui::vec2(width as f32, height as f32);
        let mut layout = Self {
            glyphs: Vec::new(),
            lines: Vec::new(),
            anchor: 0,
            end: 0,
            dragging: false,
            hovered: false,
        };
        for mut line in lines {
            align_chinese_glyphs(&mut line.glyphs);
            let start = layout.glyphs.len();
            let mut bounds = Rect::NOTHING;
            let mut glyphs = line.glyphs.into_iter().peekable();
            while let Some(glyph) = glyphs.next() {
                let [left, top, mut right, bottom] = glyph.bounds;
                // Some hOCR boxes include the characters to their right. Keep both selectable.
                if let Some(next) = glyphs.peek() {
                    let next_left = next.bounds[0];
                    if next_left > left {
                        right = right.min(next_left);
                    }
                }
                let rect = Rect::from_min_max(
                    (egui::vec2(left as f32, top as f32) * scale).to_pos2(),
                    (egui::vec2(right as f32, bottom as f32) * scale).to_pos2(),
                );
                bounds = bounds.union(rect);
                layout.glyphs.push(Glyph {
                    text: glyph.text,
                    bounds: rect,
                });
            }
            let end = layout.glyphs.len();
            if start < end {
                layout.lines.push(Line {
                    bounds,
                    range: start..end,
                });
            }
        }
        layout
    }

    pub(super) fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    pub(super) fn select_all(&mut self) {
        self.anchor = 0;
        self.end = self.glyphs.len();
    }

    fn range(&self) -> Range<usize> {
        self.anchor.min(self.end)..self.anchor.max(self.end)
    }

    pub(super) fn has_selection(&self) -> bool {
        self.anchor != self.end
    }

    pub(super) fn clear(&mut self) {
        self.end = self.anchor;
        self.dragging = false;
        self.hovered = false;
    }

    pub(super) fn dragging(&self) -> bool {
        self.dragging
    }

    pub(super) fn hovered(&self) -> bool {
        self.hovered
    }

    pub(super) fn copy(&self, ctx: &Context) -> bool {
        if !self.has_selection() {
            return false;
        }
        let range = self.range();
        let mut text = String::new();
        for line in &self.lines {
            let start = range.start.max(line.range.start);
            let end = range.end.min(line.range.end);
            if start < end {
                if !text.is_empty() {
                    text.push('\n');
                }
                for glyph in &self.glyphs[start..end] {
                    text.push_str(&glyph.text);
                }
            }
        }
        if text.is_empty() {
            return false;
        }
        ctx.copy_text(text);
        true
    }

    fn hit_line(&self, point: Pos2, margin: f32) -> Option<&Line> {
        self.lines
            .iter()
            .filter(|line| line.bounds.expand(margin).contains(point))
            .min_by(|a, b| distance(a.bounds, point).total_cmp(&distance(b.bounds, point)))
    }

    fn closest_caret(&self, point: Pos2) -> usize {
        self.lines
            .iter()
            .min_by(|a, b| distance(a.bounds, point).total_cmp(&distance(b.bounds, point)))
            .map_or(0, |line| self.caret(line, point.x))
    }

    fn caret(&self, line: &Line, x: f32) -> usize {
        let glyphs = &self.glyphs[line.range.clone()];
        let mut edge = f32::NEG_INFINITY;
        for (offset, glyph) in glyphs.iter().enumerate() {
            // The LSTM engine can report slightly overlapping character boxes.
            edge = edge.max(glyph.bounds.center().x);
            if x <= edge {
                return line.range.start + offset;
            }
        }
        line.range.end
    }

    pub(super) fn interact(&mut self, response: &Response, transform: Transform) -> bool {
        let margin = 4.0 / transform.scale;
        let (press, released, pointer, down) = response.ctx.input(|input| {
            (
                input
                    .pointer
                    .button_pressed(egui::PointerButton::Primary)
                    .then(|| input.pointer.press_origin())
                    .flatten(),
                input.pointer.button_released(egui::PointerButton::Primary),
                input.pointer.interact_pos(),
                input.pointer.primary_down(),
            )
        });
        if let Some(point) = press.filter(|_| response.contains_pointer()) {
            let point = transform.image_point(point);
            let caret = self
                .hit_line(point, margin)
                .map(|line| self.caret(line, point.x));
            self.anchor = caret.unwrap_or(0);
            self.end = self.anchor;
            self.dragging = caret.is_some();
        }
        if !self.dragging {
            return false;
        }
        if let Some(point) = pointer {
            self.end = self.closest_caret(transform.image_point(point));
        }
        self.dragging = !released && down;
        true
    }

    pub(super) fn paint(
        &mut self,
        response: &Response,
        painter: &egui::Painter,
        transform: Transform,
    ) {
        let point = response
            .hover_pos()
            .map(|point| transform.image_point(point));
        self.hovered = point.is_some_and(|p| self.hit_line(p, 4.0 / transform.scale).is_some());
        if self.dragging || self.hovered {
            response.ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
        if !self.has_selection() {
            if let Some(line) = point.and_then(|p| self.hit_line(p, 4.0 / transform.scale)) {
                painter.line_segment(
                    [
                        transform.screen_point(line.bounds.left_bottom()),
                        transform.screen_point(line.bounds.right_bottom()),
                    ],
                    egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(86, 162, 248, 120)),
                );
            }
            return;
        }
        let range = self.range();
        for line in &self.lines {
            let start = range.start.max(line.range.start);
            let end = range.end.min(line.range.end);
            if start < end {
                let selected = self.glyphs[start..end]
                    .iter()
                    .fold(Rect::NOTHING, |bounds, glyph| bounds.union(glyph.bounds));
                let rect = Rect::from_two_pos(
                    transform.screen_point(egui::pos2(selected.left(), line.bounds.top())),
                    transform.screen_point(egui::pos2(selected.right(), line.bounds.bottom())),
                );
                painter.rect_filled(
                    rect,
                    1.0,
                    Color32::from_rgba_unmultiplied(46, 130, 239, 105),
                );
            }
        }
    }
}

fn align_chinese_glyphs(glyphs: &mut [crate::ocr::Glyph]) {
    // The LSTM sometimes assigns one Han glyph the box of several neighbors.
    // For a run of full-width characters with distorted positions, distribute its boxes
    // over the detected span. Keep the original boxes for naturally aligned runs.
    let is_han = |glyph: &crate::ocr::Glyph| {
        matches!(
            glyph.text.chars().next(),
            Some('\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
        )
    };
    for run in glyphs
        .split_mut(|glyph| !is_han(glyph))
        .filter(|run| run.len() >= 3)
    {
        let left = run.iter().map(|glyph| glyph.bounds[0]).min().unwrap();
        let right = run.iter().map(|glyph| glyph.bounds[2]).max().unwrap();
        let pitch = (right - left) as f32 / run.len() as f32;
        let top = run.iter().map(|glyph| glyph.bounds[1]).max().unwrap();
        let bottom = run.iter().map(|glyph| glyph.bounds[3]).min().unwrap();
        if top >= bottom || pitch < 4.0 {
            continue;
        }
        let distorted = run.iter().enumerate().any(|(index, glyph)| {
            let center = (glyph.bounds[0] + glyph.bounds[2]) as f32 * 0.5;
            (center - (left as f32 + (index as f32 + 0.5) * pitch)).abs() > pitch * 0.36
        });
        if distorted {
            for (index, glyph) in run.iter_mut().enumerate() {
                glyph.bounds[0] = left + (index as f32 * pitch).round() as u32;
                glyph.bounds[2] = left + ((index + 1) as f32 * pitch).round() as u32;
            }
        }
    }
}

fn distance(bounds: Rect, point: Pos2) -> f32 {
    point.distance_sq(point.clamp(bounds.min, bounds.max))
}

#[derive(Clone, Copy)]
pub(super) struct Transform {
    pub(super) center: Pos2,
    pub(super) size: Vec2,
    pub(super) scale: f32,
    pub(super) rotation: u8,
    pub(super) flip_x: bool,
    pub(super) flip_y: bool,
}

impl Transform {
    fn flips(self) -> Vec2 {
        egui::vec2(
            if self.flip_x { -1.0 } else { 1.0 },
            if self.flip_y { -1.0 } else { 1.0 },
        )
    }

    fn image_point(self, point: Pos2) -> Pos2 {
        let point = super::super::rotate(
            (point - self.center) / self.scale,
            (4 - self.rotation % 4) % 4,
        );
        (point * self.flips() + self.size * 0.5).to_pos2()
    }

    fn screen_point(self, point: Pos2) -> Pos2 {
        self.center
            + super::super::rotate(
                (point.to_vec2() - self.size * 0.5) * self.flips(),
                self.rotation,
            ) * self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::Glyph as OcrGlyph;

    #[test]
    fn overlapping_chinese_boxes_keep_each_glyph_selectable() {
        let glyphs = [
            ("春", [89, 138, 135, 185]),
            ("夏", [158, 140, 199, 186]),
            ("秋", [199, 139, 231, 186]),
            ("冬", [233, 139, 471, 186]),
            ("天", [300, 134, 341, 199]),
            ("地", [340, 134, 394, 199]),
            ("山", [393, 134, 443, 199]),
            ("水", [442, 134, 475, 199]),
        ]
        .into_iter()
        .map(|(text, bounds)| OcrGlyph {
            text: text.to_owned(),
            bounds,
        })
        .collect();
        let mut layout = Layout::new(
            vec![TextLine { glyphs }],
            760,
            660,
            egui::vec2(760.0, 660.0),
        );
        layout.anchor = layout.closest_caret(egui::pos2(148.0, 155.0));
        layout.end = layout.closest_caret(egui::pos2(316.0, 155.0));
        assert_eq!(layout.range(), 1..5);
        assert_eq!(layout.glyphs[1].bounds.left(), 137.0);
        assert_eq!(layout.glyphs[4].bounds.right(), 330.0);
    }

    #[test]
    fn copies_a_reverse_cross_line_range_and_maps_rotated_glyphs() {
        let line = |text: &str, y| TextLine {
            glyphs: text
                .chars()
                .enumerate()
                .map(|(index, c)| {
                    let left = index as u32 * 10;
                    OcrGlyph {
                        text: c.to_string(),
                        bounds: [left, y, left + 10, y + 10],
                    }
                })
                .collect(),
        };
        let size = egui::vec2(80.0, 80.0);
        let mut layout = Layout::new(vec![line("中文句", 0), line("AB C", 20)], 40, 40, size);
        layout.anchor = layout.closest_caret(egui::pos2(42.0, 50.0));
        layout.end = layout.closest_caret(egui::pos2(22.0, 5.0));
        assert_eq!(layout.range(), 1..5);
        let ctx = Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| assert!(layout.copy(ctx)));
        assert!(
            matches!(output.platform_output.commands.as_slice(), [egui::OutputCommand::CopyText(text)] if text == "文句\nAB")
        );
        let transform = Transform {
            center: egui::pos2(60.0, 60.0),
            size,
            scale: 2.0,
            rotation: 1,
            flip_x: true,
            flip_y: false,
        };
        let point = egui::pos2(22.0, 5.0);
        assert_eq!(transform.image_point(transform.screen_point(point)), point);
    }
}
