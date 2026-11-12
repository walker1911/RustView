use egui::{Color32, CornerRadius, FontId, Frame, Margin, Stroke, Style, TextStyle, Visuals};

pub(super) const CANVAS: Color32 = Color32::from_rgb(23, 24, 26);
pub(super) const CHROME: Color32 = Color32::from_rgb(31, 32, 35);
pub(super) const PANEL: Color32 = Color32::from_rgb(36, 38, 41);
pub(super) const HOVER: Color32 = Color32::from_rgb(45, 47, 51);
pub(super) const SELECTED: Color32 = Color32::from_rgb(52, 55, 61);
pub(super) const BORDER: Color32 = Color32::from_rgb(48, 50, 55);
pub(super) const TEXT: Color32 = Color32::from_rgb(237, 237, 238);
pub(super) const MUTED: Color32 = Color32::from_rgb(165, 167, 173);
pub(super) const ACCENT: Color32 = Color32::from_rgb(192, 206, 225);

pub(super) fn configure(style: &mut Style) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = CANVAS;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(7);
    visuals.window_highlight_topmost = false;
    visuals.indent_has_left_vline = false;
    visuals.faint_bg_color = CHROME;
    visuals.extreme_bg_color = CANVAS;
    visuals.code_bg_color = CHROME;
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = Color32::from_rgb(224, 189, 133);
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = CHROME;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    widgets.noninteractive.fg_stroke.color = TEXT;
    widgets.inactive.bg_fill = HOVER;
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.inactive.fg_stroke.color = TEXT;
    widgets.hovered.bg_fill = SELECTED;
    widgets.hovered.weak_bg_fill = HOVER;
    widgets.hovered.bg_stroke = Stroke::NONE;
    widgets.hovered.fg_stroke.color = TEXT;
    widgets.active.bg_fill = SELECTED;
    widgets.active.weak_bg_fill = SELECTED;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    widgets.active.fg_stroke.color = TEXT;
    widgets.open.bg_fill = SELECTED;
    widgets.open.weak_bg_fill = SELECTED;
    widgets.open.bg_stroke = Stroke::NONE;
    widgets.open.fg_stroke.color = TEXT;
    for widget in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(6);
        widget.expansion = 0.0;
    }

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.text_styles.extend([
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Heading, FontId::proportional(16.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]);
}

pub(super) fn tool_group() -> Frame {
    Frame::NONE
        .fill(PANEL)
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::same(2))
}
