use super::*;

pub(super) const CONTROLS_WIDTH: f32 = 138.0;
const CAPTION_HEIGHT: f32 = 44.0;

#[derive(Clone)]
struct Gesture {
    outer: Rect,
    pointer: egui::Pos2,
}

// Keep pointer capture in egui: starting an OS modal drag can release capture
// before some backends deliver the remaining pointer events.
fn window_gesture(ui: &Ui, response: &Response, direction: Option<egui::ResizeDirection>) {
    let id = response.id.with("window-gesture");
    let ctx = ui.ctx();
    let (outer, maximized, pointer, down) = ctx.input(|i| {
        (
            i.viewport().outer_rect,
            i.viewport().maximized.unwrap_or(false),
            i.pointer.interact_pos(),
            i.pointer.primary_down(),
        )
    });
    if maximized {
        if direction.is_none() && response.dragged() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        return;
    }
    let mut gesture = ctx.data_mut(|d| d.get_temp::<Gesture>(id));
    if gesture.is_none()
        && response.is_pointer_button_down_on()
        && let (Some(outer), Some(pointer)) = (outer, pointer)
    {
        gesture = Some(Gesture {
            outer,
            pointer: outer.min + pointer.to_vec2(),
        });
    }
    if let Some(gesture) = gesture {
        if (response.dragged() || response.drag_stopped())
            && let (Some(outer), Some(pointer)) = (outer, pointer)
        {
            let delta = outer.min + pointer.to_vec2() - gesture.pointer;
            let target = gesture_rect(gesture.outer, delta, direction);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(target.min));
            if direction.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target.size()));
            }
        }
        ctx.data_mut(|d| {
            if response.drag_stopped() || !down {
                d.remove::<Gesture>(id);
            } else {
                d.insert_temp(id, gesture);
            }
        });
    }
}

fn gesture_rect(mut rect: Rect, delta: Vec2, direction: Option<egui::ResizeDirection>) -> Rect {
    use egui::ResizeDirection as D;
    let Some(direction) = direction else {
        return rect.translate(delta);
    };
    if matches!(direction, D::West | D::NorthWest | D::SouthWest) {
        rect.min.x = (rect.min.x + delta.x).min(rect.max.x - 760.0);
    }
    if matches!(direction, D::East | D::NorthEast | D::SouthEast) {
        rect.max.x = (rect.max.x + delta.x).max(rect.min.x + 760.0);
    }
    if matches!(direction, D::North | D::NorthWest | D::NorthEast) {
        rect.min.y = (rect.min.y + delta.y).min(rect.max.y - 540.0);
    }
    if matches!(direction, D::South | D::SouthWest | D::SouthEast) {
        rect.max.y = (rect.max.y + delta.y).max(rect.min.y + 540.0);
    }
    rect
}

/// Register the caption background before interactive content. Search fields,
/// navigation and caption buttons drawn afterwards retain their own hit areas.
pub(super) fn drag_region(ui: &mut Ui) {
    if ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let screen = ui.max_rect();
    let drag = Rect::from_min_max(
        screen.min + vec2(5.0, 5.0),
        pos2(screen.max.x - CONTROLS_WIDTH, screen.min.y + CAPTION_HEIGHT),
    );
    let response = ui.interact(drag, Id::new("window-drag-region"), Sense::click_and_drag());
    if response.double_clicked() {
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else {
        window_gesture(ui, &response, None);
    }
}

impl App {
    pub(super) fn window_frame(&mut self, root: &mut Ui, show_controls: bool) {
        let screen = root.max_rect();
        let ctx = root.ctx().clone();
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if show_controls {
            let l = self.settings.language;
            let controls = Rect::from_min_size(
                pos2(screen.max.x - CONTROLS_WIDTH, screen.min.y),
                vec2(CONTROLS_WIDTH, CAPTION_HEIGHT),
            );
            for index in 0..3 {
                let rect = Rect::from_min_size(
                    controls.min + vec2(index as f32 * 46.0, 0.0),
                    vec2(46.0, CAPTION_HEIGHT),
                );
                let response =
                    root.interact(rect, Id::new(("window-button", index)), Sense::click());
                let label = match index {
                    0 => l.text("最小化", "Minimize"),
                    1 if fullscreen => l.text("退出全屏", "Exit fullscreen"),
                    1 if maximized => l.text("还原窗口", "Restore window"),
                    1 => l.text("最大化", "Maximize"),
                    _ => l.text("关闭窗口", "Close window"),
                };
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                });
                if response.hovered() {
                    root.painter().rect_filled(
                        rect,
                        0,
                        if index == 2 {
                            Color32::from_rgb(182, 55, 65)
                        } else {
                            design::SELECTED
                        },
                    );
                }
                let center = rect.center();
                let stroke = Stroke::new(1.2, design::TEXT);
                match index {
                    0 => {
                        root.painter().line_segment(
                            [center + vec2(-5.0, 0.0), center + vec2(5.0, 0.0)],
                            stroke,
                        );
                    }
                    1 if maximized || fullscreen => {
                        let back =
                            Rect::from_center_size(center + vec2(2.0, -2.0), Vec2::splat(8.0));
                        root.painter()
                            .line_segment([back.left_top(), back.right_top()], stroke);
                        root.painter()
                            .line_segment([back.right_top(), back.right_bottom()], stroke);
                        root.painter().rect_stroke(
                            Rect::from_center_size(center + vec2(-1.0, 1.0), Vec2::splat(8.0)),
                            0,
                            stroke,
                            egui::StrokeKind::Inside,
                        );
                    }
                    1 => {
                        root.painter().rect_stroke(
                            Rect::from_center_size(center, Vec2::splat(10.0)),
                            0,
                            stroke,
                            egui::StrokeKind::Inside,
                        );
                    }
                    _ => {
                        root.painter().line_segment(
                            [center + vec2(-5.0, -5.0), center + vec2(5.0, 5.0)],
                            stroke,
                        );
                        root.painter().line_segment(
                            [center + vec2(5.0, -5.0), center + vec2(-5.0, 5.0)],
                            stroke,
                        );
                    }
                }
                if response.on_hover_text(label).clicked() {
                    match index {
                        0 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                        1 if fullscreen => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false))
                        }
                        1 => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                        _ => {
                            self.update_library();
                            self.library.flush();
                            self.comic.flush();
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            }
        }
        if !fullscreen && !maximized {
            root.painter().rect_stroke(
                screen,
                0,
                Stroke::new(1.0, design::LINE),
                egui::StrokeKind::Inside,
            );
            resize_edges(root);
        }
    }
}

fn resize_edges(ui: &mut Ui) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let screen = ui.max_rect();
    let edge = 5.0;
    let corner = 12.0;
    let edges = [
        (
            D::NorthWest,
            C::ResizeNwSe,
            Rect::from_min_size(screen.min, Vec2::splat(corner)),
        ),
        (
            D::NorthEast,
            C::ResizeNeSw,
            Rect::from_min_size(screen.right_top() - vec2(corner, 0.0), Vec2::splat(corner)),
        ),
        (
            D::SouthWest,
            C::ResizeNeSw,
            Rect::from_min_size(
                screen.left_bottom() - vec2(0.0, corner),
                Vec2::splat(corner),
            ),
        ),
        (
            D::SouthEast,
            C::ResizeNwSe,
            Rect::from_min_size(screen.max - Vec2::splat(corner), Vec2::splat(corner)),
        ),
        (
            D::North,
            C::ResizeVertical,
            Rect::from_min_max(
                screen.min + vec2(corner, 0.0),
                screen.right_top() + vec2(-corner, edge),
            ),
        ),
        (
            D::South,
            C::ResizeVertical,
            Rect::from_min_max(
                screen.left_bottom() + vec2(corner, -edge),
                screen.max - vec2(corner, 0.0),
            ),
        ),
        (
            D::West,
            C::ResizeHorizontal,
            Rect::from_min_max(
                screen.min + vec2(0.0, corner),
                screen.left_bottom() + vec2(edge, -corner),
            ),
        ),
        (
            D::East,
            C::ResizeHorizontal,
            Rect::from_min_max(
                screen.right_top() + vec2(-edge, corner),
                screen.max - vec2(0.0, corner),
            ),
        ),
    ];
    for (direction, cursor, rect) in edges {
        let response = ui
            .interact(
                rect,
                Id::new(("window-resize", direction as u8)),
                Sense::drag(),
            )
            .on_hover_cursor(cursor);
        window_gesture(ui, &response, Some(direction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_preserves_opposite_edges_and_minimum_size() {
        let rect = Rect::from_min_size(pos2(200.0, 100.0), vec2(1280.0, 800.0));
        assert_eq!(
            gesture_rect(rect, vec2(-50.0, 30.0), None),
            rect.translate(vec2(-50.0, 30.0))
        );
        let resized = gesture_rect(
            rect,
            vec2(1000.0, 1000.0),
            Some(egui::ResizeDirection::NorthWest),
        );
        assert_eq!(resized.max, rect.max);
        assert_eq!(resized.size(), vec2(760.0, 540.0));
        let resized = gesture_rect(
            rect,
            vec2(100.0, 80.0),
            Some(egui::ResizeDirection::SouthEast),
        );
        assert_eq!(resized.min, rect.min);
        assert_eq!(resized.size(), vec2(1380.0, 880.0));
    }

    fn drag_from_to(from: egui::Pos2, to: egui::Pos2) -> Vec<egui::ViewportCommand> {
        let ctx = egui::Context::default();
        let draw = |ui: &mut Ui| {
            drag_region(ui);
            let mut content = workspace::child(ui, "page-under-window-frame", ui.max_rect());
            content.label("Media content");
            resize_edges(ui);
        };
        let input = |events| {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
                events,
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .outer_rect = Some(Rect::from_min_size(pos2(200.0, 100.0), vec2(1280.0, 800.0)));
            input
        };
        for events in [
            vec![],
            vec![
                egui::Event::PointerMoved(from),
                egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        ] {
            let mut output = ctx.run_ui(input(events), draw);
            output.textures_delta.clear();
        }
        let mut output = ctx.run_ui(input(vec![egui::Event::PointerMoved(to)]), draw);
        let commands = output
            .viewport_output
            .values()
            .flat_map(|v| v.commands.iter().cloned())
            .collect();
        output.textures_delta.clear();
        commands
    }

    #[test]
    fn caption_drag_moves_window_and_edges_resize_without_capturing_content() {
        assert!(
            drag_from_to(pos2(850.0, 20.0), pos2(750.0, 90.0))
                .contains(&egui::ViewportCommand::OuterPosition(pos2(100.0, 170.0)))
        );
        assert!(
            drag_from_to(pos2(1278.0, 600.0), pos2(1178.0, 600.0))
                .contains(&egui::ViewportCommand::InnerSize(vec2(1180.0, 800.0)))
        );
        assert!(
            !drag_from_to(pos2(600.0, 400.0), pos2(650.0, 400.0))
                .iter()
                .any(|c| matches!(
                    c,
                    egui::ViewportCommand::OuterPosition(_) | egui::ViewportCommand::InnerSize(_)
                ))
        );
    }
}
