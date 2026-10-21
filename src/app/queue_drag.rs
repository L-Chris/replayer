use eframe::egui::{self, Pos2, Response, Ui};

const HOLD_SECONDS: f64 = 0.45;
const MOVE_TOLERANCE: f32 = 6.0;

#[derive(Default)]
pub(super) struct QueueDrag {
    press: Option<Press>,
    last_frame: Option<u64>,
    owned: bool,
}

struct Press {
    index: usize,
    source: crate::queue::Source,
    origin: Pos2,
    time: f64,
    active: bool,
}

pub(super) struct Drag {
    pub index: usize,
    pub position: Pos2,
    pub released: bool,
}

impl QueueDrag {
    pub fn begin(&mut self, ui: &Ui) {
        let frame = ui.ctx().cumulative_frame_nr();
        // A hidden queue must not resume a press when it is shown again.
        if self.last_frame.is_none_or(|last| frame > last + 1)
            || ui.input(|i| i.pointer.primary_pressed())
        {
            self.press = None;
        }
        self.last_frame = Some(frame);
        self.owned = false;
    }

    pub fn row(
        &mut self,
        ui: &Ui,
        rect: egui::Rect,
        index: usize,
        source: &crate::queue::Source,
    ) -> Response {
        // Keep the cover/play and remove controls out of the reorder hit area.
        let rect = egui::Rect::from_min_max(
            rect.min + egui::vec2(48.0, 0.0),
            rect.max - egui::vec2(40.0, 0.0),
        );
        let response = ui.interact(rect, ui.id().with("row"), egui::Sense::click_and_drag());
        if response.is_pointer_button_down_on()
            && response.ctx.input(|i| i.pointer.primary_pressed())
            && let Some(origin) = response.interact_pointer_pos()
        {
            self.press = Some(Press {
                index,
                source: source.clone(),
                origin,
                time: response.ctx.input(|i| i.time),
                active: false,
            });
        }
        if let Some(press) = &self.press
            && press.index == index
            && &press.source == source
        {
            self.owned = response.is_pointer_button_down_on()
                || response.ctx.input(|i| i.pointer.primary_released());
        }
        response
    }

    pub fn finish(&mut self, ui: &Ui) -> Option<Drag> {
        let (down, released, position, time, cancelled) = ui.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos(),
                i.time,
                !i.focused || i.key_pressed(egui::Key::Escape),
            )
        });
        if cancelled || !self.owned || (!down && !released) || position.is_none() {
            self.press = None;
            return None;
        }
        let press = self.press.as_mut()?;
        let position = position?;
        if !press.active {
            // Movement before the hold completes is a click/scroll gesture,
            // never a delayed reorder after the pointer has already moved.
            if released || position.distance(press.origin) > MOVE_TOLERANCE {
                self.press = None;
                return None;
            }
            let remaining = HOLD_SECONDS - (time - press.time);
            if remaining > 0.0 {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(remaining));
                return None;
            }
            press.active = true;
        }
        let drag = Drag {
            index: press.index,
            position,
            released,
        };
        if released {
            self.press = None;
        }
        Some(drag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Rect, Sense, pos2, vec2};

    #[derive(Default)]
    struct Harness {
        ctx: egui::Context,
        gesture: QueueDrag,
        clicks: usize,
    }

    impl Harness {
        fn frame(&mut self, time: f64, events: Vec<Event>) -> Option<Drag> {
            let mut drag = None;
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 200.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    self.gesture.begin(ui);
                    let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(240.0, 48.0));
                    self.gesture
                        .row(ui, rect, 0, &crate::queue::Source::File("song.flac".into()));
                    let cover = ui.interact(
                        Rect::from_min_size(pos2(18.0, 18.0), vec2(32.0, 32.0)),
                        ui.id().with("cover"),
                        Sense::click(),
                    );
                    if cover.clicked() {
                        self.clicks += 1;
                    }
                    drag = self.gesture.finish(ui);
                },
            );
            output.textures_delta.clear();
            drag
        }

        fn press(&mut self, position: Pos2) {
            self.frame(0.0, vec![]);
            assert!(
                self.frame(
                    0.01,
                    vec![Event::PointerMoved(position), button(position, true)]
                )
                .is_none()
            );
        }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    #[test]
    fn quick_click_and_early_motion_never_start_reordering() {
        let mut h = Harness::default();
        h.press(pos2(100.0, 30.0));
        assert!(
            h.frame(0.08, vec![Event::PointerMoved(pos2(103.0, 32.0))])
                .is_none()
        );
        assert!(
            h.frame(0.12, vec![button(pos2(103.0, 32.0), false)])
                .is_none()
        );
        let mut h = Harness::default();
        h.press(pos2(100.0, 30.0));
        assert!(
            h.frame(0.1, vec![Event::PointerMoved(pos2(100.0, 55.0))])
                .is_none()
        );
        assert!(h.frame(0.8, vec![]).is_none());
        assert!(
            h.frame(0.9, vec![button(pos2(100.0, 90.0), false)])
                .is_none()
        );
    }

    #[test]
    fn holding_arms_reorder_and_only_release_finishes_it() {
        let mut h = Harness::default();
        h.press(pos2(100.0, 30.0));
        assert!(h.frame(0.4, vec![]).is_none());
        let drag = h
            .frame(0.48, vec![])
            .expect("stationary long press should arm");
        assert_eq!(drag.index, 0);
        assert!(!drag.released);
        let drag = h
            .frame(0.6, vec![Event::PointerMoved(pos2(100.0, 140.0))])
            .unwrap();
        assert!(!drag.released);
        assert_eq!(drag.position.y, 140.0);
        assert!(
            h.frame(0.7, vec![button(pos2(100.0, 140.0), false)])
                .unwrap()
                .released
        );
        assert!(h.frame(0.8, vec![]).is_none());
    }

    #[test]
    fn cover_clicks_do_not_arm_reorder_even_when_held() {
        let mut h = Harness::default();
        h.press(pos2(30.0, 30.0));
        assert!(h.frame(0.5, vec![]).is_none());
        assert!(
            h.frame(0.6, vec![button(pos2(30.0, 30.0), false)])
                .is_none()
        );
        assert_eq!(h.clicks, 1);
    }

    #[test]
    fn escape_cancels_an_armed_drag() {
        let mut h = Harness::default();
        h.press(pos2(100.0, 30.0));
        assert!(h.frame(0.5, vec![]).is_some());
        assert!(
            h.frame(
                0.6,
                vec![Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default()
                }]
            )
            .is_none()
        );
        assert!(
            h.frame(0.7, vec![button(pos2(100.0, 140.0), false)])
                .is_none()
        );
    }
}
