use super::*;

pub(super) const BAR_HEIGHT: f32 = 48.0;

pub(super) struct BarLayout {
    pub footer: Rect,
    options: Rect,
    pager: Rect,
    zoom: Rect,
    chapters: Rect,
    pub status: Rect,
    compact: bool,
    has_status: bool,
}

pub(super) fn layout(ui: &Ui, screen: Rect, l: Language, has_status: bool) -> BarLayout {
    let labels = [
        display_name(Display::Original, l),
        display_name(Display::Translated, l),
        display_name(Display::Compare, l),
    ];
    let display_width = design::segmented_width(ui, &labels);
    let options_width = display_width + 8.0 + design::CONTROL_HEIGHT + 6.0 + design::CONTROL_HEIGHT;
    arrange(screen, options_width, has_status)
}

fn arrange(screen: Rect, options_width: f32, has_status: bool) -> BarLayout {
    let h = design::CONTROL_HEIGHT;
    let footer = Rect::from_min_max(pos2(screen.min.x, screen.max.y - BAR_HEIGHT), screen.max);
    let cy = footer.center().y;
    let right_slot = Rect::from_center_size(pos2(footer.max.x - 16.0 - h * 0.5, cy), vec2(h, h));
    let status = right_slot;
    let chapters = if has_status {
        right_slot.translate(vec2(-(h + 8.0), 0.0))
    } else {
        right_slot
    };
    let zoom_width = h * 2.0 + 56.0 + 8.0;
    let zoom = Rect::from_min_size(
        pos2(chapters.min.x - 8.0 - zoom_width, cy - h * 0.5),
        vec2(zoom_width, h),
    );
    let available = zoom.min.x - 16.0 - (footer.min.x + 16.0);
    let compact = available < options_width + 16.0 + 336.0;
    let options = Rect::from_min_size(
        pos2(footer.min.x + 16.0, cy - h * 0.5),
        vec2(if compact { h } else { options_width }, h),
    );
    let start = options.max.x + 16.0;
    let end = zoom.min.x - 16.0;
    let pager_width = (if compact { 276.0_f32 } else { 336.0_f32 }).min((end - start).max(160.0));
    let pager = Rect::from_center_size(pos2((start + end) * 0.5, cy), vec2(pager_width, h));
    BarLayout {
        footer,
        options,
        pager,
        zoom,
        chapters,
        status,
        compact,
        has_status,
    }
}

pub(super) struct Status<'a> {
    pub message: &'a str,
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub working: bool,
}

impl Status<'_> {
    pub(super) fn has_activity(&self, has_page_errors: bool) -> bool {
        self.working
            || self.total > 0
            || self.failed > 0
            || !self.message.is_empty()
            || has_page_errors
    }
}

#[derive(Default)]
pub(super) struct Actions {
    pub chapter: Option<usize>,
    pub translate: Option<bool>,
    pub retry: bool,
    pub cancel: bool,
}

pub(super) fn show(
    ui: &mut Ui,
    reader: &mut Reader,
    bar: &BarLayout,
    neighbors: [Option<usize>; 2],
    l: Language,
    visible: bool,
    status: Status<'_>,
) -> Actions {
    let mut actions = Actions::default();
    if visible {
        ui.painter().rect_filled(bar.footer, 0, BACKGROUND);
        ui.painter().line_segment(
            [bar.footer.left_top(), bar.footer.right_top()],
            Stroke::new(1.0, design::LINE),
        );
        let mut options = reader_slot(ui, "reader-options", bar.options);
        if bar.compact {
            let response = design::icon_button(
                &mut options,
                icons::TRANSLATE,
                l.text("显示与翻译", "Display & translation"),
                reader.display != Display::Original,
            );
            egui::Popup::menu(&response).show(|ui| {
                ui.set_min_width(168.0);
                for display in [Display::Original, Display::Translated, Display::Compare] {
                    if design::menu_value(ui, &mut reader.display, display, display_name(display, l))
                        .changed()
                    {
                        reader.restore = true;
                        ui.close();
                    }
                }
                ui.separator();
                ui.add_enabled_ui(!reader.urls.is_empty(), |ui| {
                    for (all, label) in [
                        (false, l.text("翻译本页", "Translate page")),
                        (true, l.text("翻译本章", "Translate chapter")),
                    ] {
                        if design::menu_item(ui, false, label).clicked() {
                            actions.translate = Some(all);
                            ui.close();
                        }
                    }
                });
            });
        } else {
            options.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let displays = [Display::Original, Display::Translated, Display::Compare];
                let labels = displays.map(|d| display_name(d, l));
                let mut selected = displays
                    .iter()
                    .position(|d| *d == reader.display)
                    .unwrap_or(0);
                if design::segmented(ui, &mut selected, &labels) {
                    reader.display = displays[selected];
                    reader.restore = true;
                }
                ui.add_enabled_ui(!reader.urls.is_empty(), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if design::icon_button(
                        ui,
                        icons::TRANSLATE,
                        l.text("翻译本页", "Translate page"),
                        false,
                    )
                    .clicked()
                    {
                        actions.translate = Some(false);
                    }
                    if design::icon_button(
                        ui,
                        icons::IMAGES,
                        l.text("翻译本章", "Translate chapter"),
                        false,
                    )
                    .clicked()
                    {
                        actions.translate = Some(true);
                    }
                });
            });
        }
        let mut zoom = reader_slot(ui, "reader-zoom", bar.zoom);
        let old_zoom = reader.zoom;
        zoom.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if design::icon_button(ui, icons::MINUS, l.text("缩小", "Zoom out"), false).clicked() {
                reader.zoom = (reader.zoom - 0.1).max(0.5);
            }
            if design::control(
                ui,
                &format!("{:.0}%", reader.zoom * 100.0),
                None,
                design::Control::Quiet,
                56.0,
            )
            .on_hover_text(l.text("恢复 100%", "Reset zoom"))
            .clicked()
            {
                reader.zoom = 1.0;
            }
            if design::icon_button(ui, icons::PLUS, l.text("放大", "Zoom in"), false).clicked() {
                reader.zoom = (reader.zoom + 0.1).min(2.0);
            }
        });
        if old_zoom != reader.zoom {
            reader.restore = true;
            reader.dirty = true;
        }
        let mut chapters = reader_slot(ui, "reader-chapters-toggle", bar.chapters);
        if design::icon_button(
            &mut chapters,
            icons::SIDEBAR_SIMPLE,
            l.text("章节目录", "Chapters"),
            reader.sidebar,
        )
        .clicked()
        {
            reader.sidebar = !reader.sidebar;
            reader.sidebar_follow = true;
        }
        pager(ui, reader, bar.pager, neighbors, l, &mut actions);
    }
    if bar.has_status {
        let mut status_ui = reader_slot(ui, "reader-status", bar.status);
        let response = design::icon_button(&mut status_ui, icons::INFO, "", status.working);
        let has_error =
            status.failed > 0 || !status.message.is_empty() || !reader.errors.is_empty();
        if status.working || has_error {
            ui.painter().circle_filled(
                response.rect.right_top() + vec2(-6.0, 6.0),
                3.0,
                if has_error {
                    Color32::LIGHT_RED
                } else {
                    ACCENT
                },
            );
        }
        let response = response.on_hover_ui(|ui| {
            ui.set_max_width(280.0);
            status_text(ui, &status, !reader.errors.is_empty(), l);
        });
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(180.0);
            ui.set_max_width(280.0);
            status_text(ui, &status, !reader.errors.is_empty(), l);
            if status.working || !reader.errors.is_empty() {
                ui.separator();
            }
            if status.working
                && design::menu_item(ui, false, l.text("取消翻译", "Cancel translation")).clicked()
            {
                actions.cancel = true;
                ui.close();
            }
            if !reader.errors.is_empty()
                && design::menu_item(
                    ui,
                    false,
                    l.text("重新加载本章图片", "Reload chapter images"),
                )
                .clicked()
            {
                actions.retry = true;
                ui.close();
            }
        });
    }
    actions
}

fn pager(
    ui: &mut Ui,
    reader: &mut Reader,
    rect: Rect,
    neighbors: [Option<usize>; 2],
    l: Language,
    actions: &mut Actions,
) {
    let h = design::CONTROL_HEIGHT;
    let cy = rect.center().y;
    let mut previous = reader_slot(
        ui,
        "reader-previous",
        Rect::from_min_size(rect.min, vec2(h, h)),
    );
    previous.add_enabled_ui(neighbors[0].is_some(), |ui| {
        if design::icon_button_sized(
            ui,
            h,
            1.0,
            false,
            l.text("上一章", "Previous chapter"),
            draw_prev,
        )
        .clicked()
        {
            actions.chapter = neighbors[0];
        }
    });
    let mut next = reader_slot(
        ui,
        "reader-next",
        Rect::from_min_size(pos2(rect.max.x - h, rect.min.y), vec2(h, h)),
    );
    next.add_enabled_ui(neighbors[1].is_some(), |ui| {
        if design::icon_button_sized(
            ui,
            h,
            1.0,
            false,
            l.text("下一章", "Next chapter"),
            draw_next,
        )
        .clicked()
        {
            actions.chapter = neighbors[1];
        }
    });
    let count = reader.urls.len();
    ui.painter().text(
        pos2(rect.max.x - 75.0, cy),
        egui::Align2::CENTER_CENTER,
        format!(
            "{} / {}",
            if count > 0 { reader.current + 1 } else { 0 },
            count
        ),
        egui::FontId::proportional(12.0),
        MUTED,
    );
    let track_rect = Rect::from_min_max(
        pos2(rect.min.x + 44.0, cy - 14.0),
        pos2(rect.max.x - 114.0, cy + 14.0),
    );
    let mut track_ui = reader_slot(ui, "reader-page-track", track_rect);
    let fraction = if count > 1 {
        reader.current as f32 / (count - 1) as f32
    } else {
        0.0
    };
    let (track, response) = hslider(
        &mut track_ui,
        track_rect.width(),
        28.0,
        fraction,
        3.0,
        false,
        1.0,
    );
    if count > 0
        && (response.clicked() || response.dragged())
        && let Some(p) = response.interact_pointer_pos()
    {
        reader.current = (((p.x - track.min.x) / track.width()).clamp(0.0, 1.0)
            * (count - 1) as f32)
            .round() as usize;
        reader.fraction = 0.0;
        reader.restore = true;
        reader.dirty = true;
    }
}

fn display_name(display: Display, l: Language) -> &'static str {
    match display {
        Display::Original => l.text("原文", "Original"),
        Display::Translated => l.text("译文", "Translated"),
        Display::Compare => l.text("对照", "Compare"),
    }
}

fn status_text(ui: &mut Ui, status: &Status<'_>, has_page_errors: bool, l: Language) {
    if !status.message.is_empty() {
        ui.label(status.message);
    }
    if status.total > 0 {
        ui.label(format!(
            "{} {}/{}",
            if status.working {
                l.text("翻译中", "Translating")
            } else {
                l.text("翻译完成", "Translation finished")
            },
            status.done,
            status.total
        ));
        if status.failed > 0 {
            ui.colored_label(
                Color32::LIGHT_RED,
                format!("{} {}", status.failed, l.text("页失败", "pages failed")),
            );
        }
    } else if has_page_errors && status.message.is_empty() {
        ui.colored_label(
            Color32::LIGHT_RED,
            l.text("部分图片加载失败", "Some images failed to load"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_do_not_overlap_or_touch_borders_at_supported_window_sizes() {
        for width in [628.0, 760.0, 960.0, 1100.0, 1280.0, 1920.0] {
            for options_width in [288.0, 340.0] {
                for has_status in [false, true] {
                    let screen = Rect::from_min_size(pos2(70.0, 35.0), vec2(width, 540.0));
                    let bar = arrange(screen, options_width, has_status);
                    assert_eq!(bar.footer.height(), 48.0);
                    let mut controls = vec![bar.options, bar.pager, bar.zoom, bar.chapters];
                    if has_status {
                        controls.push(bar.status);
                    }
                    for (index, rect) in controls.iter().enumerate() {
                        assert!(bar.footer.contains_rect(*rect));
                        assert!(
                            rect.min.y >= bar.footer.min.y + 5.0
                                && rect.max.y <= bar.footer.max.y - 5.0,
                            "control {index} too close to footer border: {rect:?} in {:?}",
                            bar.footer
                        );
                        for other in &controls[index + 1..] {
                            assert!(!rect.intersects(*other));
                        }
                    }
                }
            }
        }
    }
}
