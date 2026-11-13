use super::{theme, Viewer};
use egui::{Color32, Context, Id, Rect};
use std::time::{Duration, Instant};

const EDGE: f32 = 18.0;
const ENTER_DELAY: f64 = 0.075;
const LEAVE_DELAY: f64 = 0.55;
const DRAG_GRACE: Duration = Duration::from_millis(700);

pub(super) struct State {
    edge_since: Option<f64>,
    hide_at: Option<f64>,
    drag_end: Option<Instant>,
    shown: bool,
    top: Rect,
    bottom: Rect,
    pub(super) menu_open: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            edge_since: None,
            hide_at: None,
            drag_end: None,
            shown: false,
            top: Rect::NOTHING,
            bottom: Rect::NOTHING,
            menu_open: false,
        }
    }
}

impl State {
    fn opacity_id() -> Id {
        Id::new("quiet-chrome-fade")
    }

    pub(super) fn reset(&mut self, ctx: &Context) {
        *self = Self::default();
        ctx.animate_bool_with_time(Self::opacity_id(), false, 0.0);
    }

    fn animate(&mut self, ctx: &Context, motion: (bool, Option<Instant>, bool)) -> f32 {
        let (dragging, drag_end, caption_hover) = motion;
        let (now, pointer, down) =
            ctx.input(|i| (i.time, i.pointer.hover_pos(), i.pointer.any_down()));
        let screen = ctx.screen_rect();
        let at_edge = caption_hover
            || pointer.is_some_and(|p| screen.contains(p) && p.y - screen.top() < EDGE);
        let in_controls = pointer.is_some_and(|p| self.top.contains(p) || self.bottom.contains(p));
        let hold = dragging || self.menu_open || self.shown && in_controls;
        if drag_end != self.drag_end {
            self.drag_end = drag_end;
            if let Some(end) = drag_end {
                self.shown = true;
                self.hide_at = Some(now + DRAG_GRACE.saturating_sub(end.elapsed()).as_secs_f64());
            }
        }
        self.advance(ctx, now, at_edge && !down, hold);
        ctx.animate_bool_with_time_and_easing(
            Self::opacity_id(),
            self.shown,
            if self.shown { 0.16 } else { 0.23 },
            egui::emath::easing::quadratic_out,
        )
    }

    fn advance(&mut self, ctx: &Context, now: f64, at_edge: bool, hold: bool) {
        if hold || at_edge {
            self.hide_at = None;
            let elapsed = now - *self.edge_since.get_or_insert(now);
            if hold || elapsed >= ENTER_DELAY {
                self.shown = true;
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(ENTER_DELAY - elapsed));
            }
        } else {
            self.edge_since = None;
            if self.shown {
                let delay = *self.hide_at.get_or_insert(now + LEAVE_DELAY) - now;
                if delay <= 0.0 {
                    self.shown = false;
                } else {
                    ctx.request_repaint_after(Duration::from_secs_f64(delay));
                }
            }
        }
    }
}

impl Viewer {
    pub(super) fn draw_quiet_chrome(&mut self, ctx: &Context) -> bool {
        #[cfg(windows)]
        let motion = self
            .native_keys
            .as_ref()
            .map_or((false, None, false), |keys| keys.window_motion());
        #[cfg(not(windows))]
        let motion = (false, None, false);
        let opacity = self.quiet_chrome.animate(ctx, motion);
        if opacity <= 0.0 {
            self.quiet_chrome.top = Rect::NOTHING;
            self.quiet_chrome.bottom = Rect::NOTHING;
            self.quiet_chrome.menu_open = false;
            return false;
        }

        let screen = ctx.screen_rect();
        let canvas = self.extra.viewport;
        let top = screen.min - egui::vec2(0.0, 6.0 * (1.0 - opacity));
        let bottom = egui::pos2(screen.left(), canvas.bottom() + 4.0 * (1.0 - opacity));
        let fill = Color32::from_rgba_unmultiplied(
            theme::CHROME.r(),
            theme::CHROME.g(),
            theme::CHROME.b(),
            215,
        );
        if self.extra.toolbar {
            let old_menu_open = self.quiet_chrome.menu_open;
            let result = overlay(
                "quiet-toolbar",
                ctx,
                top,
                egui::Align2::LEFT_TOP,
                self.quiet_chrome.shown,
                |ui| {
                    ui.set_opacity(opacity);
                    egui::Frame::NONE
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(12, 6))
                        .show(ui, |ui| self.toolbar_contents(ui, ctx))
                        .inner
                },
            );
            self.quiet_chrome.top = result.response.rect.intersect(screen);
            self.quiet_chrome.menu_open = result.inner;
            if old_menu_open != result.inner {
                ctx.request_repaint();
            }
        } else {
            self.quiet_chrome.top = Rect::NOTHING;
            self.quiet_chrome.menu_open = false;
        }
        if self.extra.statusbar {
            let result = overlay(
                "quiet-status",
                ctx,
                bottom,
                egui::Align2::LEFT_BOTTOM,
                self.quiet_chrome.shown,
                |ui| {
                    ui.set_opacity(opacity);
                    ui.set_clip_rect(canvas);
                    egui::Frame::NONE
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(12, 5))
                        .show(ui, |ui| self.status_contents(ui, ctx));
                },
            );
            self.quiet_chrome.bottom = result.response.rect.intersect(canvas);
        } else {
            self.quiet_chrome.bottom = Rect::NOTHING;
        }
        true
    }
}

fn overlay<R>(
    id: &str,
    ctx: &Context,
    pos: egui::Pos2,
    pivot: egui::Align2,
    interactable: bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let screen = ctx.screen_rect();
    egui::Area::new(Id::new(id))
        .order(egui::Order::Middle)
        .fixed_pos(pos)
        .pivot(pivot)
        .default_width(screen.width())
        .constrain(false)
        .movable(false)
        .fade_in(false)
        .interactable(interactable)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.set_width(screen.width());
            if !interactable {
                ui.disable();
            }
            contents(ui)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_waits_at_edge_and_cancels_hiding_when_menu_is_held() {
        let ctx = Context::default();
        let mut state = State::default();
        state.advance(&ctx, 0.0, true, false);
        state.advance(&ctx, 0.03, false, false);
        state.advance(&ctx, 0.08, true, false);
        assert!(!state.shown);
        state.advance(&ctx, 0.17, true, false);
        assert!(state.shown);
        state.advance(&ctx, 0.2, false, false);
        state.advance(&ctx, 0.3, false, true);
        state.advance(&ctx, 1.2, false, true);
        assert!(state.shown);
        state.advance(&ctx, 1.21, false, false);
        state.advance(&ctx, 1.71, false, false);
        assert!(state.shown);
        state.advance(&ctx, 1.8, false, false);
        assert!(!state.shown);
    }
}
