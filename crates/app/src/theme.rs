//! The app's look: a dark, calm UI that lets the map's colours speak.
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Visuals,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(14, 16, 20);
pub const PANEL: Color32 = Color32::from_rgb(21, 24, 30);
pub const CARD: Color32 = Color32::from_rgb(29, 33, 41);
pub const CARD_HOVER: Color32 = Color32::from_rgb(38, 43, 53);
pub const BORDER: Color32 = Color32::from_rgb(42, 47, 58);
pub const TEXT: Color32 = Color32::from_rgb(228, 231, 237);
pub const MUTED: Color32 = Color32::from_rgb(134, 142, 157);
pub const ACCENT: Color32 = Color32::from_rgb(98, 204, 112);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(44, 104, 56);
pub const WARN: Color32 = Color32::from_rgb(240, 180, 70);
pub const NEW: Color32 = Color32::from_rgb(255, 112, 88);

const BOLD: &str = "semibold";

/// The semibold UI font at a size.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

/// Inter for the interface, with egui's own fonts behind it for symbols.
fn fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    let mut bold = vec!["inter-semibold".to_owned()];
    bold.extend(fallback);
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
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
            (TextStyle::Heading, bold(18.0)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
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
            .font(bold(10.5))
            .extra_letter_spacing(0.8)
            .color(MUTED),
    );
}

/// A small filled pill with text (badges like NEW or 26.3).
pub fn pill(ui: &mut egui::Ui, text: &str, fill: Color32, ink: Color32) -> egui::Response {
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).font(bold(10.5)).color(ink));
        })
        .response
}
