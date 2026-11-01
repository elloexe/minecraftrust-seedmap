//! The app's look: a dark, calm UI that lets the map's colours speak.
use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Visuals};

pub const BG: Color32 = Color32::from_rgb(17, 19, 24);
pub const PANEL: Color32 = Color32::from_rgb(23, 26, 32);
pub const CARD: Color32 = Color32::from_rgb(31, 35, 43);
pub const CARD_HOVER: Color32 = Color32::from_rgb(40, 45, 55);
pub const BORDER: Color32 = Color32::from_rgb(46, 51, 62);
pub const TEXT: Color32 = Color32::from_rgb(222, 226, 233);
pub const MUTED: Color32 = Color32::from_rgb(140, 148, 162);
pub const ACCENT: Color32 = Color32::from_rgb(92, 196, 108);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(52, 120, 64);
pub const WARN: Color32 = Color32::from_rgb(240, 180, 70);
pub const NEW: Color32 = Color32::from_rgb(255, 128, 96);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = CARD;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    let w = &mut visuals.widgets;
    for (state, fill) in [
        (&mut w.noninteractive, PANEL),
        (&mut w.inactive, CARD),
        (&mut w.hovered, CARD_HOVER),
        (&mut w.active, ACCENT_DIM),
        (&mut w.open, CARD_HOVER),
    ] {
        state.corner_radius = CornerRadius::same(6);
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.window_margin = Margin::same(12);
        style.spacing.interact_size.y = 26.0;
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(19.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.5, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// A rounded card frame for sidebar sections.
pub fn card() -> egui::Frame {
    egui::Frame::NONE
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .small()
            .strong()
            .color(MUTED),
    );
}
