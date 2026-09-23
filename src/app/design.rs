use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, vec2};

pub const BACKGROUND: Color32 = Color32::from_rgb(14, 17, 23);
pub const SURFACE: Color32 = Color32::from_rgb(24, 29, 39);
pub const ACCENT: Color32 = Color32::from_rgb(86, 133, 245);
pub const MUTED: Color32 = Color32::from_rgb(151, 162, 181);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(Color32::from_rgb(232, 237, 246));
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.window_highlight_topmost = false;
    style.visuals.window_corner_radius = CornerRadius::same(16);
    style.visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(48, 57, 73));
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = Color32::from_rgb(30, 36, 48);
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.hyperlink_color = ACCENT;
    for widgets in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widgets.corner_radius = CornerRadius::same(8);
        widgets.bg_stroke = Stroke::new(1.0, Color32::from_rgb(53, 64, 82));
        // Inactive buttons drop the frame stroke, so expansion must compensate
        // for it or hover changes the widget size and neighbours jump.
        widgets.expansion = widgets.bg_stroke.width;
    }
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 41, 55);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 58, 80);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(49, 72, 114);
    style.spacing.item_spacing = vec2(12.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 8.0);
    style.spacing.interact_size = vec2(34.0, 34.0);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(22.0));
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}
