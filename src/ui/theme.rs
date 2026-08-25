use eframe::egui::{self, Color32, Frame, RichText, Rounding, Stroke, Vec2};

#[allow(dead_code)]
pub mod colors {
    use eframe::egui::Color32;

    // Base ultra-sleek dark slate background & panel surfaces
    pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0E, 0x17);
    pub const PANEL: Color32 = Color32::from_rgb(0x14, 0x1B, 0x26);
    pub const PANEL_ALT: Color32 = Color32::from_rgb(0x0F, 0x15, 0x20);
    pub const CARD_BG: Color32 = Color32::from_rgb(0x1E, 0x26, 0x36);
    pub const CARD_HOVER: Color32 = Color32::from_rgb(0x27, 0x33, 0x48);
    pub const BORDER: Color32 = Color32::from_rgb(0x2C, 0x38, 0x4E);
    pub const BORDER_HI: Color32 = Color32::from_rgb(0x40, 0x51, 0x70);

    // Text scale
    pub const TEXT: Color32 = Color32::from_rgb(0xF8, 0xFA, 0xFC);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x82, 0x92, 0xA8);

    // Status colors
    pub const ACCENT: Color32 = Color32::from_rgb(0x63, 0x66, 0xF1); // Indigo #6366F1
    pub const ACCENT_BG: Color32 = Color32::from_rgb(0x22, 0x25, 0x54);
    pub const OK: Color32 = Color32::from_rgb(0x10, 0xB9, 0x81); // Emerald Green
    pub const OK_BG: Color32 = Color32::from_rgb(0x0A, 0x2E, 0x22);
    pub const WARN: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B); // Amber
    pub const WARN_BG: Color32 = Color32::from_rgb(0x38, 0x22, 0x08);
    pub const ERR: Color32 = Color32::from_rgb(0xF4, 0x3F, 0x5E); // Rose Red
    pub const ERR_BG: Color32 = Color32::from_rgb(0x3B, 0x12, 0x19);

    // Buttons & Interactivity
    pub const BTN_BG: Color32 = Color32::from_rgb(0x1E, 0x26, 0x36);
    pub const BTN_HOVER: Color32 = Color32::from_rgb(0x2C, 0x38, 0x4E);
    pub const PENDING: Color32 = Color32::from_rgb(0x47, 0x55, 0x69);
    pub const BTN_LABEL_ON_ACCENT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    // White text on the bright OK/WARN/ERR fills fails WCAG contrast badly
    // (as low as ~2.1:1 for WARN) since those are light, saturated colors.
    // Dark label text on those fills reads at ~5.2-9:1 instead.
    pub const BTN_LABEL_ON_LIGHT: Color32 = BG;
}

#[allow(dead_code)]
pub mod metrics {
    pub const ROUNDING_SM: f32 = 4.0;
    pub const ROUNDING_MD: f32 = 6.0;
    pub const ROUNDING_LG: f32 = 8.0;

    pub const SPACE_XS: f32 = 4.0;
    pub const SPACE_SM: f32 = 8.0;
    pub const SPACE_MD: f32 = 12.0;
    pub const SPACE_LG: f32 = 16.0;

    pub const FS_HEADER: f32 = 15.0;
    pub const FS_TITLE: f32 = 13.0;
    pub const FS_BODY: f32 = 12.0;
    pub const FS_LABEL: f32 = 11.0;
    pub const FS_SMALL: f32 = 10.0;

    pub const MAX_CONTENT_WIDTH: f32 = 1280.0;
    pub const ACTION_BAR_HEIGHT: f32 = 40.0;
    pub const MIN_CONSOLE_HEIGHT: f32 = 110.0;
    pub const OUTER_MARGIN: f32 = 16.0;
}

pub fn panel_frame() -> Frame {
    Frame::none()
        .fill(colors::PANEL)
        .stroke(Stroke::new(1.0_f32, colors::BORDER))
        .rounding(Rounding::same(metrics::ROUNDING_LG))
        .inner_margin(metrics::SPACE_MD)
}

pub fn chip_frame() -> Frame {
    Frame::none()
        .fill(colors::PANEL_ALT)
        .stroke(Stroke::new(1.0_f32, colors::BORDER))
        .rounding(Rounding::same(metrics::ROUNDING_SM))
        .inner_margin(Vec2::new(8.0, 5.0))
}

/// A recessed inset panel for embedded scrollable/editable content areas
/// (e.g. the activity log body, the JSON config text editor). Shared by
/// console.rs and json_config_modal.rs, which previously hand-rolled this
/// exact same `Frame::none()` combination independently.
pub fn inset_frame() -> Frame {
    Frame::none()
        .fill(colors::PANEL_ALT)
        .stroke(Stroke::new(1.0_f32, colors::BORDER))
        .rounding(Rounding::same(metrics::ROUNDING_MD))
        .inner_margin(8.0)
}

/// Blend a color toward white by `amount` (0.0 = unchanged, 1.0 = white).
fn brighten(c: Color32, amount: f32) -> Color32 {
    let lerp = |v: u8| -> u8 { (v as f32 + (255.0 - v as f32) * amount).round() as u8 };
    Color32::from_rgb(lerp(c.r()), lerp(c.g()), lerp(c.b()))
}

/// Blend a color toward black by `amount` (0.0 = unchanged, 1.0 = black).
fn darken(c: Color32, amount: f32) -> Color32 {
    let lerp = |v: u8| -> u8 { (v as f32 * (1.0 - amount)).round() as u8 };
    Color32::from_rgb(lerp(c.r()), lerp(c.g()), lerp(c.b()))
}

pub fn flow_button(
    ui: &mut egui::Ui,
    label: &str,
    fill: Color32,
    text_color: Color32,
) -> egui::Response {
    ui.scope(|ui| {
        ui.style_mut().spacing.button_padding = Vec2::new(14.0, 8.0);
        // Set per-state fills instead of a single hardcoded `.fill()` so the
        // button actually brightens/darkens on hover/press like every other
        // interactive widget in the app, rather than only reacting via its
        // (much subtler) border stroke.
        let widgets = &mut ui.style_mut().visuals.widgets;
        widgets.inactive.weak_bg_fill = fill;
        widgets.hovered.weak_bg_fill = brighten(fill, 0.12);
        widgets.active.weak_bg_fill = darken(fill, 0.08);
        ui.add(
            egui::Button::new(
                RichText::new(label)
                    .size(metrics::FS_BODY)
                    .strong()
                    .color(text_color),
            )
            .rounding(Rounding::same(metrics::ROUNDING_MD)),
        )
    })
    .inner
}

pub fn apply_custom_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = colors::BG;
    visuals.window_fill = colors::PANEL;
    visuals.faint_bg_color = colors::PANEL_ALT;
    visuals.extreme_bg_color = colors::PANEL_ALT;

    visuals.widgets.noninteractive.bg_fill = colors::CARD_BG;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, colors::BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
    visuals.widgets.noninteractive.rounding = Rounding::same(metrics::ROUNDING_MD);

    visuals.widgets.inactive.bg_fill = colors::BTN_BG;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, colors::BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
    visuals.widgets.inactive.rounding = Rounding::same(metrics::ROUNDING_MD);

    visuals.widgets.hovered.bg_fill = colors::BTN_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, colors::BORDER_HI);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
    visuals.widgets.hovered.rounding = Rounding::same(metrics::ROUNDING_MD);

    visuals.widgets.active.bg_fill = colors::PANEL;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, colors::ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
    visuals.widgets.active.rounding = Rounding::same(metrics::ROUNDING_MD);

    visuals.selection.bg_fill = colors::ACCENT_BG;
    visuals.selection.stroke = Stroke::new(1.0_f32, colors::ACCENT);

    ctx.set_visuals(visuals);
}
