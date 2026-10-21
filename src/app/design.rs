use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, vec2};

pub const BACKGROUND: Color32 = Color32::from_rgb(17, 19, 21);
pub const SURFACE: Color32 = Color32::from_rgb(25, 29, 32);
pub const ACCENT: Color32 = Color32::from_rgb(169, 224, 205);
pub const MUTED: Color32 = Color32::from_rgb(146, 154, 157);
pub const TEXT: Color32 = Color32::from_rgb(238, 240, 236);
pub const LINE: Color32 = Color32::from_rgb(42, 48, 50);
pub const BORDER: Color32 = Color32::from_rgb(62, 71, 74);
pub const SELECTED: Color32 = Color32::from_rgb(37, 55, 50);

/// Crop artwork to fill a tile without stretching, using the same corner radius
/// for music covers, video thumbnails and compact transport artwork.
pub fn cover(ui: &egui::Ui, texture: &egui::TextureHandle, rect: egui::Rect, radius: u8) {
    let size = texture.size_vec2();
    let image_ratio = size.x / size.y.max(1.0);
    let tile_ratio = rect.width() / rect.height().max(1.0);
    let uv_size = if image_ratio > tile_ratio {
        vec2(tile_ratio / image_ratio, 1.0)
    } else {
        vec2(1.0, image_ratio / tile_ratio)
    };
    let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), uv_size);
    egui::Image::new((texture.id(), rect.size()))
        .uv(uv)
        .corner_radius(radius)
        .paint_at(ui, rect);
}

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.window_highlight_topmost = false;
    style.visuals.window_corner_radius = CornerRadius::same(12);
    style.visuals.window_stroke = Stroke::new(1.0, LINE);
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.selection.bg_fill = SELECTED;
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.slider_trailing_fill = true;
    for widgets in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widgets.corner_radius = CornerRadius::same(8);
        widgets.bg_stroke = Stroke::new(1.0, BORDER);
        // Frames must stay inside their allocated bounds, including in the first
        // row of a clipped child or scroll viewport. Keep every state identical.
        widgets.expansion = 0.0;
    }
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(37, 44, 45);
    style.visuals.widgets.active.weak_bg_fill = SELECTED;
    style.visuals.widgets.open.weak_bg_fill = SELECTED;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(58, 69, 66);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(37, 44, 45);
    style.visuals.widgets.active.bg_fill = SELECTED;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    style.spacing.item_spacing = vec2(12.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 8.0);
    style.spacing.interact_size = vec2(34.0, 34.0);
    style.spacing.slider_rail_height = 4.0;
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 6.0,
        handle_min_length: 32.0,
        bar_inner_margin: 8.0,
        bar_outer_margin: 2.0,
        content_margin: egui::Margin::same(2),
        ..egui::style::ScrollStyle::solid()
    };
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
        .insert(TextStyle::Heading, FontId::proportional(28.0));
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub type Icon = fn(&egui::Painter, egui::Rect, Color32);

#[derive(Clone, Copy)]
pub enum Control {
    Outline,
    Quiet,
    Circle,
}

/// Toolbar controls share a fixed hit height and paint entirely inside it.
pub fn control(
    ui: &mut egui::Ui,
    label: &str,
    icon: Option<Icon>,
    kind: Control,
    width: f32,
) -> egui::Response {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.into(), font, TEXT);
    let icon_space = if icon.is_some() && !label.is_empty() {
        24.0
    } else {
        0.0
    };
    let width = if width > 0.0 {
        width
    } else {
        (galley.size().x + icon_space + 24.0).max(34.0)
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let hovered = response.hovered() || response.has_focus();
    let fill = if hovered { SELECTED } else { SURFACE };
    match kind {
        Control::Circle => {
            ui.painter().circle_filled(rect.center(), 16.0, fill);
            ui.painter()
                .circle_stroke(rect.center(), 15.5, Stroke::new(1.0, BORDER));
        }
        Control::Outline => {
            ui.painter().rect_filled(rect, 8, fill);
            ui.painter().rect_stroke(
                rect,
                8,
                Stroke::new(1.0, if hovered { ACCENT } else { BORDER }),
                egui::StrokeKind::Inside,
            );
        }
        Control::Quiet if hovered => {
            ui.painter().rect_filled(rect, 7, SURFACE);
        }
        Control::Quiet => {}
    }
    let color = if hovered {
        TEXT
    } else {
        Color32::from_rgb(204, 211, 211)
    };
    if let Some(icon) = icon {
        let center = if label.is_empty() {
            rect.center()
        } else {
            egui::pos2(rect.min.x + 20.0, rect.center().y)
        };
        icon(
            ui.painter(),
            egui::Rect::from_center_size(center, vec2(16.0, 16.0)),
            color,
        );
    }
    if !label.is_empty() {
        let x = if icon.is_some() {
            rect.min.x + 34.0
        } else {
            rect.center().x - galley.size().x * 0.5
        };
        let painter = ui
            .painter()
            .with_clip_rect(rect.shrink(4.0).intersect(ui.clip_rect()));
        painter.galley_with_override_text_color(
            egui::pos2(x, rect.center().y - galley.size().y * 0.5),
            galley,
            color,
        );
    }
    response
}

pub fn segmented(ui: &mut egui::Ui, selected: &mut usize, labels: &[&str]) -> bool {
    let total = segmented_width(ui, labels);
    let width = (total - 4.0) / labels.len().max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 34.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 17, SURFACE);
    ui.painter()
        .rect_stroke(rect, 17, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
    let mut changed = false;
    for (index, label) in labels.iter().enumerate() {
        let segment = egui::Rect::from_min_size(
            rect.min + vec2(2.0 + index as f32 * width, 2.0),
            vec2(width, 30.0),
        );
        let response = ui.interact(
            segment,
            ui.id().with(("segment", index)),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                ui.is_enabled(),
                index == *selected,
                *label,
            )
        });
        if index == *selected || response.hovered() {
            ui.painter().rect_filled(
                segment,
                15,
                if index == *selected { ACCENT } else { SELECTED },
            );
        }
        ui.painter().text(
            segment.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            FontId::proportional(13.0),
            if index == *selected { BACKGROUND } else { TEXT },
        );
        if response.clicked() {
            changed = *selected != index;
            *selected = index;
        }
    }
    changed
}

pub fn segmented_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let label_width = labels
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap((*label).into(), FontId::proportional(13.0), TEXT)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    (label_width + 28.0).max(66.0) * labels.len() as f32 + 4.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_row_button_border_is_not_clipped_at_different_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            apply(&ctx);
            ctx.set_pixels_per_point(scale);
            for hovering in [false, true] {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(400.0, 160.0),
                        )),
                        events: if hovering {
                            vec![egui::Event::PointerMoved(egui::pos2(40.0, 25.0))]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ui| {
                        let rect =
                            egui::Rect::from_min_size(egui::pos2(20.0, 16.0), vec2(250.0, 40.0));
                        let mut row = crate::app::workspace::child(ui, "clipped-button-row", rect);
                        let _ = row.button("Translate chapter");
                    },
                );
                let mut borders = 0;
                for shape in &output.shapes {
                    if let egui::Shape::Rect(rect) = &shape.shape
                        && rect.stroke.width > 0.0
                    {
                        assert!(
                            shape.clip_rect.contains_rect(rect.rect),
                            "scale={scale}: {:?} outside {:?}",
                            rect.rect,
                            shape.clip_rect
                        );
                        borders += 1;
                    }
                }
                assert!(borders > 0);
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn scrolling_reserves_a_gutter_instead_of_covering_full_width_rows() {
        let ctx = egui::Context::default();
        apply(&ctx);
        for time in [0.0, 0.2, 0.4, 0.8] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(400.0, 160.0),
                    )),
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    let outer = ui.available_rect_before_wrap();
                    let result = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        )
                        .show(ui, |ui| {
                            let mut right = 0.0_f32;
                            for _ in 0..20 {
                                let (row, _) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 32.0),
                                    egui::Sense::hover(),
                                );
                                right = right.max(row.max.x);
                            }
                            right
                        });
                    if time > 0.4 {
                        assert!(
                            outer.max.x - result.inner_rect.max.x >= 14.0,
                            "missing gutter: {:?} within {:?}",
                            result.inner_rect,
                            outer
                        );
                        assert!(result.inner <= result.inner_rect.max.x);
                    }
                },
            );
            output.textures_delta.clear();
        }
    }
}
