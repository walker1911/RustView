use super::{floating_toolbar_glass::Glass, Action, Viewer};
use egui::{Color32, Context, Event, FontId, Id, Pos2, Rect, Sense, Stroke, Vec2};

const HEIGHT: f32 = 43.0;
const BUTTON_WIDTH: f32 = 34.0;
const GAP: f32 = 7.0;
const INSET: f32 = 6.0;
const WIDTH: f32 = 6.0 * BUTTON_WIDTH + 5.0 * GAP + 2.0 * INSET;
const CORNER: u8 = 13;
const TOOLS: [(Action, &str); 6] = [
    (Action::Previous, "上一张（←）"),
    (Action::Next, "下一张（→）"),
    (Action::ZoomOut, "缩小（↓）"),
    (Action::ZoomIn, "放大（↑）"),
    (Action::ActualSize, "原始尺寸（1:1）"),
    (Action::RotateRight, "顺时针旋转 90°"),
];

fn area_id() -> Id {
    Id::new("floating-image-toolbar")
}

fn button_id(index: usize) -> Id {
    area_id().with(index)
}

#[derive(Default)]
pub(super) struct State {
    until: Option<f64>,
    shown: bool,
    glass: Glass,
}

impl State {
    pub(super) fn has_focus(&self, ctx: &Context) -> bool {
        self.shown
            && ctx.memory(|memory| {
                memory
                    .focused()
                    .is_some_and(|id| (0..TOOLS.len()).any(|index| id == button_id(index)))
            })
    }

    fn hide(&mut self, ctx: &Context) {
        self.until = None;
        self.shown = false;
        ctx.memory_mut(|memory| {
            if let Some(id) = memory
                .focused()
                .filter(|id| (0..TOOLS.len()).any(|index| *id == button_id(index)))
            {
                memory.surrender_focus(id);
            }
        });
    }

    fn visible(&mut self, ctx: &Context, image: Rect, bar: Rect) -> bool {
        let (time, cursor, moved, pressed, origin, escaped) = ctx.input(|input| {
            (
                input.time,
                input.pointer.hover_pos(),
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::PointerMoved(_))),
                input.pointer.any_down(),
                input.pointer.press_origin(),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if escaped || pressed && !(self.shown && origin.is_some_and(|p| bar.contains(p))) {
            self.hide(ctx);
            return false;
        }
        let over_bar = cursor.is_some_and(|pos| bar.contains(pos));
        let over_image = cursor.is_some_and(|pos| image.contains(pos));
        if moved && over_image {
            self.until = Some(time + 1.4);
        } else if self.shown && !over_image && !over_bar {
            self.until = self.until.map(|until| until.min(time + 0.21));
        }
        if self.shown && (over_bar || pressed) {
            return true;
        }
        if let Some(delay) = self
            .until
            .map(|until| until - time)
            .filter(|delay| *delay > 0.0)
        {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay));
            return true;
        }
        self.hide(ctx);
        false
    }
}

impl Viewer {
    pub(super) fn draw_floating_toolbar(&mut self, ctx: &Context, chrome_visible: bool) {
        let canvas = self.extra.viewport;
        let has_image = self.current.is_some();
        let blocked = chrome_visible
            || !self.extra.toolbar
            || self.browser
            || self.extra.compare > 0
            || self.extra.edit_dialog.is_some()
            || self.password_dialog
            || self.archive_jump.open
            || self.recent_ui.open
            || self.extra.dialog.is_some()
            || self.error.is_some()
            || self.help
            || self.properties
            || self.extra.settings_ui.open
            || self.extra.print_ui.open
            || ctx.memory(|memory| memory.any_popup_open());
        let state = &mut self.floating_toolbar;
        let fade_id = area_id().with("fade");
        if blocked
            || !has_image && !self.extra.presentation.has_frame()
            || canvas.width() < WIDTH + 12.0
            || canvas.height() < HEIGHT + 18.0
            || !ctx.input(|input| input.focused)
        {
            state.hide(ctx);
            ctx.animate_bool_with_time(fade_id, false, 0.0);
            return;
        }

        let mut bar = Rect::from_center_size(
            egui::pos2(canvas.center().x, canvas.bottom() - 13.0 - HEIGHT / 2.0),
            egui::vec2(WIDTH, HEIGHT),
        );
        let image_bounds = if let Some(image) = &self.current {
            let size = egui::vec2(image.width as f32, image.height as f32);
            let size = if self.rotation % 2 == 0 {
                size
            } else {
                size.yx()
            };
            Rect::from_center_size(canvas.center() + self.pan, size * self.scale).intersect(canvas)
        } else {
            self.extra
                .presentation
                .frame_bounds(canvas, ctx.pixels_per_point())
        };
        let visible = state.visible(ctx, image_bounds, bar);
        state.shown = visible;
        let opacity = ctx.animate_bool_with_time(fade_id, visible, 0.16);
        if opacity <= 0.0 {
            return;
        }
        bar = bar.translate(egui::vec2(0.0, 5.0 * (1.0 - opacity)));
        let previous = self.navigation_target(false, false).is_some();
        let next = self.navigation_target(true, false).is_some();
        let actual = !self.fit && (self.target_scale * ctx.pixels_per_point() - 1.0).abs() < 0.001;
        let state = &mut self.floating_toolbar;
        let chosen = egui::Area::new(area_id())
            .order(egui::Order::Foreground)
            .fixed_pos(bar.min)
            .default_size(bar.size())
            .constrain(false)
            .movable(false)
            .fade_in(false)
            .interactable(visible)
            .show(ctx, |ui| {
                ui.set_min_size(bar.size());
                ui.set_clip_rect(canvas);
                ui.set_opacity(opacity);
                let painter = ui.painter();
                if let Some(image) = &self.current {
                    state.glass.paint(
                        painter,
                        bar,
                        image,
                        canvas.center() + self.pan,
                        self.scale,
                        self.rotation,
                        self.flip_x,
                        self.flip_y,
                        self.extra.background_tile.is_some() || self.photo_soft_focus,
                    );
                }
                painter.rect_filled(bar, CORNER, Color32::from_rgba_unmultiplied(18, 20, 22, 36));

                let mut chosen = None;
                for (index, (action, label)) in TOOLS.into_iter().enumerate() {
                    let center = egui::pos2(
                        bar.left()
                            + INSET
                            + BUTTON_WIDTH / 2.0
                            + index as f32 * (BUTTON_WIDTH + GAP),
                        bar.center().y,
                    );
                    let hit = Rect::from_center_size(center, egui::vec2(BUTTON_WIDTH, 35.0));
                    let enabled = match action {
                        Action::Previous => previous,
                        Action::Next => next,
                        _ => has_image,
                    };
                    let response = ui.interact(
                        hit,
                        button_id(index),
                        if visible && enabled {
                            Sense::click()
                        } else {
                            Sense::hover()
                        },
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label)
                    });
                    let highlighted =
                        enabled && visible && (response.hovered() || response.has_focus());
                    let painter = ui.painter();
                    if highlighted || action == Action::ActualSize && actual {
                        painter.rect_filled(
                            hit,
                            8,
                            Color32::from_white_alpha(if highlighted { 20 } else { 30 }),
                        );
                    }
                    let alpha = if !enabled {
                        69
                    } else if highlighted {
                        240
                    } else {
                        184
                    };
                    paint_icon(
                        painter,
                        center,
                        action,
                        Color32::from_rgba_unmultiplied(242, 242, 244, alpha),
                    );
                    if index + 1 < TOOLS.len() {
                        let x = hit.right() + GAP / 2.0;
                        painter.rect_filled(
                            Rect::from_center_size(egui::pos2(x, center.y), egui::vec2(1.0, 12.0)),
                            0,
                            Color32::from_rgba_unmultiplied(244, 244, 246, 56),
                        );
                    }
                    if visible {
                        let response = response.on_hover_text(label);
                        if enabled && response.clicked() {
                            chosen = Some(action);
                        }
                    }
                }
                chosen
            })
            .inner;
        if let Some(action) = chosen {
            self.perform(action, ctx);
        }
    }
}

fn paint_icon(painter: &egui::Painter, center: Pos2, action: Action, color: Color32) {
    let stroke = Stroke::new(1.8_f32, color);
    let point = |x, y| center + Vec2::new(x, y);
    match action {
        Action::Previous | Action::Next => {
            let direction = if action == Action::Previous {
                1.0
            } else {
                -1.0
            };
            painter.add(egui::Shape::line(
                vec![
                    point(2.6 * direction, -5.0),
                    point(-2.4 * direction, 0.0),
                    point(2.6 * direction, 5.0),
                ],
                stroke,
            ));
        }
        Action::ZoomOut | Action::ZoomIn => {
            painter.line_segment([point(-5.0, 0.0), point(5.0, 0.0)], stroke);
            if action == Action::ZoomIn {
                painter.line_segment([point(0.0, -5.0), point(0.0, 5.0)], stroke);
            }
        }
        Action::ActualSize => {
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "1:1",
                FontId::monospace(13.0),
                color,
            );
        }
        Action::RotateRight => {
            let arc = (0..=24)
                .map(|step| {
                    let angle = (-22.0_f32 - step as f32 * 12.5).to_radians();
                    point(6.0 * angle.cos(), 6.0 * angle.sin())
                })
                .collect();
            painter.add(egui::Shape::line(arc, stroke));
            painter.add(egui::Shape::line(
                vec![point(6.0, -6.3), point(6.0, -2.3), point(2.0, -2.3)],
                stroke,
            ));
        }
        _ => unreachable!("floating toolbar action"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_motion_over_photo_reveals_the_toolbar_and_other_areas_do_not() {
        let ctx = Context::default();
        let mut state = State::default();
        let image = Rect::from_min_max(egui::pos2(150.0, 60.0), egui::pos2(470.0, 400.0));
        let bar = Rect::from_min_size(egui::pos2(184.0, 387.0), egui::vec2(WIDTH, HEIGHT));
        let mut step = |time, event: Option<Event>, expected| {
            let input = egui::RawInput {
                time: Some(time),
                events: event.into_iter().collect(),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                let visible = state.visible(ctx, image, bar);
                assert_eq!(visible, expected, "time {time}");
                state.shown = visible;
            });
        };

        step(
            0.0,
            Some(Event::PointerMoved(egui::pos2(90.0, 180.0))),
            false,
        );
        step(0.1, Some(Event::PointerMoved(bar.center())), false);
        step(
            0.2,
            Some(Event::PointerMoved(egui::pos2(380.0, 90.0))),
            true,
        );
        step(1.7, None, false);
        step(
            1.8,
            Some(Event::PointerMoved(egui::pos2(300.0, 250.0))),
            true,
        );
        step(1.9, Some(Event::PointerMoved(bar.center())), true);
        step(3.5, None, true);
        step(
            3.6,
            Some(Event::PointerMoved(egui::pos2(310.0, 465.0))),
            false,
        );
        step(
            3.7,
            Some(Event::PointerMoved(egui::pos2(310.0, 470.0))),
            false,
        );
    }
}
