use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, vec2};

pub const BACKGROUND: Color32 = Color32::from_rgb(17, 19, 21);
pub const SURFACE: Color32 = Color32::from_rgb(25, 29, 32);
pub const ACCENT: Color32 = Color32::from_rgb(169, 224, 205);
pub const MUTED: Color32 = Color32::from_rgb(146, 154, 157);
pub const TEXT: Color32 = Color32::from_rgb(238, 240, 236);
pub const LINE: Color32 = Color32::from_rgb(42, 48, 50);
pub const BORDER: Color32 = Color32::from_rgb(62, 71, 74);
pub const SELECTED: Color32 = Color32::from_rgb(37, 55, 50);
pub const FIELD: Color32 = Color32::from_rgb(30, 35, 39);
pub const HOVER: Color32 = Color32::from_rgb(32, 43, 40);
pub const CONTROL_HEIGHT: f32 = 34.0;
pub const CARD_PADDING: f32 = 8.0;

/// Keep the frame in every state: egui's frameless inactive selectable loses
/// its stroke margins, making the label and its neighbours move on hover.
pub fn selectable_value<'a, T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    let selected = *current == value;
    let mut response = ui
        .scope(|ui| {
            if !selected {
                let inactive = &mut ui.visuals_mut().widgets.inactive;
                inactive.weak_bg_fill = Color32::TRANSPARENT;
                inactive.bg_stroke.color = Color32::TRANSPARENT;
            }
            ui.add(egui::Button::selectable(selected, label).frame_when_inactive(true))
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() && !selected {
        *current = value;
        response.mark_changed();
    }
    response
}

pub fn control_width(ui: &egui::Ui, label: &str, with_icon: bool) -> f32 {
    let text =
        ui.painter()
            .layout_no_wrap(label.into(), TextStyle::Button.resolve(ui.style()), TEXT);
    (text.size().x
        + if with_icon && !label.is_empty() {
            24.0
        } else {
            0.0
        }
        + 24.0)
        .ceil()
        .max(CONTROL_HEIGHT)
}

pub fn combo_width(ui: &egui::Ui, label: &str) -> f32 {
    let text =
        ui.painter()
            .layout_no_wrap(label.into(), TextStyle::Button.resolve(ui.style()), TEXT);
    (text.size().x
        + 2.0 * ui.spacing().button_padding.x
        + ui.spacing().icon_width
        + ui.spacing().icon_spacing)
        .ceil()
}

pub fn card_content(rect: egui::Rect) -> egui::Rect {
    rect.shrink(CARD_PADDING)
}

/// Reserve the same gutter for short and long lists, so switching tabs never
/// changes the content width or the number of card columns.
pub fn scroll() -> egui::ScrollArea {
    egui::ScrollArea::vertical()
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .auto_shrink([false, false])
}

pub fn text_edit(value: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(value)
        .font(FontId::proportional(14.0))
        .margin(egui::Margin::symmetric(12, 7))
        .min_size(vec2(40.0, CONTROL_HEIGHT))
        .vertical_align(egui::Align::Center)
        .desired_width(f32::INFINITY)
        .background_color(FIELD)
}

pub fn search_field(
    ui: &mut egui::Ui,
    value: &mut String,
    width: f32,
    hint: &str,
) -> egui::Response {
    let mut response = ui.add(
        text_edit(value)
            .hint_text(hint)
            .desired_width(width)
            .margin(egui::Margin {
                left: 36,
                right: 34,
                top: 7,
                bottom: 7,
            }),
    );
    super::draw_search(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(response.rect.min.x + 18.0, response.rect.center().y),
            vec2(15.0, 15.0),
        ),
        MUTED,
    );
    if !value.is_empty() {
        let rect = egui::Rect::from_center_size(
            egui::pos2(response.rect.max.x - 17.0, response.rect.center().y),
            vec2(26.0, 26.0),
        );
        let clear = ui
            .interact(rect, response.id.with("clear"), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        super::draw_close(
            ui.painter(),
            rect.shrink(7.0),
            if clear.hovered() { TEXT } else { MUTED },
        );
        if clear.clicked() {
            value.clear();
            response.mark_changed();
            response.request_focus();
        }
    }
    response
}

pub fn heart(p: &egui::Painter, rect: egui::Rect, color: Color32, filled: bool) {
    let side = rect.width().min(rect.height());
    let rect = egui::Rect::from_center_size(rect.center(), vec2(side, side));
    let curves = [
        [(0.5, 0.91), (0.39, 0.82), (0.06, 0.58), (0.06, 0.31)],
        [(0.06, 0.31), (0.06, 0.07), (0.34, 0.02), (0.5, 0.23)],
        [(0.5, 0.23), (0.66, 0.02), (0.94, 0.07), (0.94, 0.31)],
        [(0.94, 0.31), (0.94, 0.58), (0.61, 0.82), (0.5, 0.91)],
    ];
    let mut points = Vec::with_capacity(40);
    for curve in curves {
        for step in 0..10 {
            let t = step as f32 / 10.0;
            let u = 1.0 - t;
            let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let (mut x, mut y) = (0.0, 0.0);
            for (point, weight) in curve.into_iter().zip(weights) {
                x += point.0 * weight;
                y += point.1 * weight;
            }
            points.push(rect.min + vec2(x * side, y * side));
        }
    }
    p.add(egui::epaint::PathShape {
        points,
        closed: true,
        fill: if filled { color } else { Color32::TRANSPARENT },
        stroke: Stroke::new(1.5, color).into(),
    });
}

pub fn favorite_at(
    ui: &egui::Ui,
    id: egui::Id,
    rect: egui::Rect,
    selected: bool,
) -> egui::Response {
    let side = rect.width().min(rect.height());
    let rect = egui::Rect::from_center_size(rect.center(), vec2(side, side));
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled(),
            selected,
            "Favorite",
        )
    });
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 7, SELECTED);
    }
    heart(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), vec2(20.0, 20.0)),
        if selected { ACCENT } else { TEXT },
        selected,
    );
    response
}

/// Paint before the card contents so hover changes only the background.
pub fn card_hover(ui: &egui::Ui, response: &egui::Response, radius: u8) {
    if response.contains_pointer() && ui.is_enabled() {
        ui.painter().rect_filled(response.rect, radius, HOVER);
    }
    response
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand);
}

pub fn tabs(ui: &mut egui::Ui, selected: &mut usize, labels: &[&str]) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, LINE),
    );
    let mut x = rect.min.x;
    let mut changed = false;
    for (index, label) in labels.iter().enumerate() {
        let text = ui.painter().layout_no_wrap(
            (*label).into(),
            FontId::proportional(14.0),
            if *selected == index { ACCENT } else { MUTED },
        );
        let width = (text.size().x + 20.0).max(54.0);
        let tab = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), vec2(width, 36.0));
        let response = ui
            .interact(tab, ui.id().with(("tab", index)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                *selected == index,
                *label,
            )
        });
        if response.hovered() {
            ui.painter()
                .rect_filled(tab.shrink2(vec2(0.0, 3.0)), 6, SURFACE);
        }
        ui.painter().galley(
            egui::pos2(
                tab.center().x - text.size().x * 0.5,
                tab.center().y - text.size().y * 0.5,
            ),
            text,
            TEXT,
        );
        if *selected == index {
            ui.painter().line_segment(
                [
                    egui::pos2(x + 8.0, rect.max.y - 1.0),
                    egui::pos2(x + width - 8.0, rect.max.y - 1.0),
                ],
                Stroke::new(2.0, ACCENT),
            );
        }
        if response.clicked() {
            changed |= *selected != index;
            *selected = index;
        }
        x += width + 6.0;
    }
    changed
}

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
    style.visuals.widgets.inactive.weak_bg_fill = FIELD;
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
    // Let dropdown captions determine their width instead of the default 200pt.
    style.spacing.combo_width = 0.0;
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
    Primary,
    Outline,
    Quiet,
    Circle,
}

/// Navigation back actions use the same icon-only target in every workspace.
pub fn back_button(ui: &mut egui::Ui, tooltip: &str) -> egui::Response {
    let response = control(
        ui,
        "",
        Some(super::draw_back),
        Control::Quiet,
        CONTROL_HEIGHT,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    response.on_hover_text(tooltip)
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
    let width = if width > 0.0 {
        width
    } else {
        control_width(ui, label, icon.is_some())
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let hovered = response.hovered() || response.has_focus();
    let fill = if hovered { SELECTED } else { SURFACE };
    match kind {
        Control::Primary => {
            ui.painter().rect_filled(
                rect,
                8,
                if hovered {
                    ACCENT.gamma_multiply(1.08)
                } else {
                    ACCENT
                },
            );
        }
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
    let color = if matches!(kind, Control::Primary) {
        BACKGROUND
    } else if hovered {
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
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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
        let response = ui
            .interact(
                segment,
                ui.id().with(("segment", index)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
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
    fn selectable_hover_and_selection_preserve_text_and_neighbour_positions() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            apply(&ctx);
            ctx.set_pixels_per_point(scale);
            let mut selected = 1;
            let mut baseline = None;
            let mut targets = [egui::Pos2::ZERO; 2];
            // Include a second frame per state: native buttons use last frame's
            // interaction state when allocating their frame and text.
            for phase in 0..8 {
                if phase == 6 {
                    selected = 0;
                }
                for _ in 0..2 {
                    let mut bounds = Vec::new();
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                vec2(600.0, 200.0),
                            )),
                            events: match phase {
                                1 | 2 | 6 => vec![egui::Event::PointerMoved(targets[0])],
                                3 | 4 | 7 => vec![egui::Event::PointerMoved(targets[1])],
                                _ => vec![egui::Event::PointerGone],
                            },
                            ..Default::default()
                        },
                        |ui| {
                            ui.horizontal(|ui| {
                                bounds.push(
                                    selectable_value(ui, &mut selected, 0, "Local music").rect,
                                );
                                bounds.push(
                                    selectable_value(ui, &mut selected, 1, "Online search").rect,
                                );
                                bounds.push(ui.button("Next control").rect);
                            });
                        },
                    );
                    targets = [bounds[0].center(), bounds[1].center()];
                    let text_positions: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape {
                                Some(text.pos)
                            } else {
                                None
                            }
                        })
                        .collect();
                    let geometry = (bounds, text_positions);
                    if let Some(baseline) = &baseline {
                        assert_eq!(
                            &geometry, baseline,
                            "layout shifted at scale={scale}, phase={phase}"
                        );
                    } else {
                        baseline = Some(geometry);
                    }
                    output.textures_delta.clear();
                }
            }
        }
    }

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
                    let result = scroll().show(ui, |ui| {
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

    #[test]
    fn switching_between_empty_and_long_lists_preserves_content_width() {
        for width in [354.0, 628.0, 1030.0] {
            let ctx = egui::Context::default();
            apply(&ctx);
            let mut widths = Vec::new();
            for (frame, count) in [0, 1, 60, 0, 60].into_iter().enumerate() {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(width, 240.0),
                        )),
                        time: Some(frame as f64),
                        ..Default::default()
                    },
                    |ui| {
                        let result = scroll().id_salt(("tab", frame)).show(ui, |ui| {
                            let available = ui.available_width();
                            for _ in 0..count {
                                ui.allocate_exact_size(vec2(available, 48.0), egui::Sense::hover());
                            }
                            available
                        });
                        widths.push(result.inner);
                    },
                );
                output.textures_delta.clear();
            }
            assert!(
                widths.iter().all(|value| (value - widths[0]).abs() < 0.1),
                "content shifted at {width}: {widths:?}"
            );
        }
    }

    #[test]
    fn search_fields_password_fields_and_buttons_share_height_and_baseline() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            apply(&ctx);
            ctx.set_pixels_per_point(scale);
            let mut value = String::new();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(600.0, 180.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ui.horizontal(|ui| {
                        let field = search_field(ui, &mut value, 220.0, "Search comics");
                        let button = control(ui, "Search", None, Control::Primary, 80.0);
                        assert!(
                            (field.rect.height() - CONTROL_HEIGHT).abs() < 0.1,
                            "scale={scale}: {:?}",
                            field.rect
                        );
                        assert!((field.rect.center().y - button.rect.center().y).abs() < 0.1);
                        assert!(!field.rect.intersects(button.rect));
                    });
                    let password = ui.add(text_edit(&mut value).password(true));
                    assert!((password.rect.height() - CONTROL_HEIGHT).abs() < 0.1);
                },
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn heart_keeps_its_proportions_in_wide_and_tall_slots() {
        for size in [vec2(16.0, 30.0), vec2(30.0, 16.0), vec2(20.0, 20.0)] {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                heart(
                    ui.painter(),
                    egui::Rect::from_min_size(egui::pos2(20.0, 20.0), size),
                    ACCENT,
                    false,
                )
            });
            let bounds = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Path(path) = &shape.shape {
                        Some(egui::Rect::from_points(&path.points))
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(
                (bounds.width() / bounds.height() - 1.0).abs() < 0.12,
                "distorted heart: {bounds:?}"
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn favorite_click_does_not_open_the_parent_card_and_hover_shows_a_hand() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut opened = 0;
        let mut favorites = 0;
        let position = egui::pos2(112.0, 42.0);
        for events in [
            vec![],
            vec![egui::Event::PointerMoved(position)],
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(300.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let rect =
                        egui::Rect::from_min_size(egui::pos2(20.0, 20.0), vec2(120.0, 200.0));
                    let card = ui.interact(rect, ui.id().with("card"), egui::Sense::click());
                    card_hover(ui, &card, 8);
                    let fav = favorite_at(
                        ui,
                        ui.id().with("favorite"),
                        egui::Rect::from_center_size(position, vec2(32.0, 32.0)),
                        false,
                    );
                    if card.clicked() {
                        opened += 1;
                    }
                    if fav.clicked() {
                        favorites += 1;
                    }
                },
            );
            if ctx.input(|i| i.pointer.hover_pos().is_some()) {
                assert_eq!(
                    output.platform_output.cursor_icon,
                    egui::CursorIcon::PointingHand
                );
            }
            output.textures_delta.clear();
        }
        assert_eq!(opened, 0);
        assert_eq!(favorites, 1);
    }
}
