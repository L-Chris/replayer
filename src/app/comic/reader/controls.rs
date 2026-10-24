use super::*;

pub(super) const BAR_HEIGHT: f32 = 36.0;

pub(super) struct BarLayout {
    pub footer: Rect,
    options: Rect,
    pager: Rect,
    zoom: Rect,
    chapters: Rect,
    pub status: Rect,
    compact: bool,
    mode_width: f32,
    display_width: f32,
}

pub(super) fn layout(ui: &Ui, screen: Rect, l: Language) -> BarLayout {
    let mode_width = [
        ReadingMode::Vertical,
        ReadingMode::LeftToRight,
        ReadingMode::RightToLeft,
    ]
    .map(|mode| design::control_width(ui, mode_name(mode, l), true) + 24.0)
    .into_iter()
    .fold(0.0_f32, f32::max);
    let display_width = [Display::Original, Display::Translated, Display::Compare]
        .map(|display| design::control_width(ui, display_name(display, l), false) + 24.0)
        .into_iter()
        .fold(0.0_f32, f32::max);
    let options_width = mode_width
        + display_width
        + design::control_width(ui, l.text("翻译本页", "Translate page"), true)
        + design::control_width(ui, l.text("翻译本章", "Translate chapter"), true)
        + 24.0;
    arrange(screen, options_width, mode_width, display_width)
}

fn arrange(screen: Rect, options_width: f32, mode_width: f32, display_width: f32) -> BarLayout {
    let compact = screen.width() < options_width + 336.0 + 220.0 + 64.0;
    let footer = Rect::from_min_max(pos2(screen.min.x, screen.max.y - BAR_HEIGHT), screen.max);
    let cy = footer.center().y;
    let status = Rect::from_center_size(pos2(footer.max.x - 33.0, cy), vec2(34.0, 34.0));
    let chapters = status.translate(vec2(-42.0, 0.0));
    let zoom = Rect::from_min_size(pos2(chapters.min.x - 144.0, cy - 17.0), vec2(132.0, 34.0));
    let options = Rect::from_min_size(
        pos2(footer.min.x + 16.0, cy - 17.0),
        vec2(if compact { 34.0 } else { options_width }, 34.0),
    );
    let start = options.max.x + 16.0;
    let end = zoom.min.x - 16.0;
    let pager = Rect::from_center_size(
        pos2((start + end) * 0.5, cy),
        vec2(if compact { 276.0 } else { 336.0 }, 34.0),
    );
    BarLayout {
        footer,
        options,
        pager,
        zoom,
        chapters,
        status,
        compact,
        mode_width,
        display_width,
    }
}

pub(super) struct Status<'a> {
    pub message: &'a str,
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub working: bool,
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
            let response = design::control(
                &mut options,
                "",
                Some(reader_book),
                design::Control::Quiet,
                34.0,
            )
            .on_hover_text(l.text("阅读与翻译设置", "Reading and translation"));
            egui::Popup::menu(&response).show(|ui| {
                ui.label(l.text("阅读方式", "Reading mode"));
                for mode in [
                    ReadingMode::Vertical,
                    ReadingMode::LeftToRight,
                    ReadingMode::RightToLeft,
                ] {
                    if design::selectable_value(ui, &mut reader.mode, mode, mode_name(mode, l))
                        .changed()
                    {
                        reader.restore = true;
                        reader.dirty = true;
                        ui.close();
                    }
                }
                ui.separator();
                ui.label(l.text("显示模式", "Display mode"));
                for display in [Display::Original, Display::Translated, Display::Compare] {
                    if design::selectable_value(
                        ui,
                        &mut reader.display,
                        display,
                        display_name(display, l),
                    )
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
                        if ui.button(label).clicked() {
                            actions.translate = Some(all);
                            ui.close();
                        }
                    }
                });
            });
        } else {
            options.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let mode = dropdown(
                    ui,
                    mode_name(reader.mode, l),
                    Some(reader_book),
                    bar.mode_width,
                );
                egui::Popup::menu(&mode).show(|ui| {
                    for mode in [
                        ReadingMode::Vertical,
                        ReadingMode::LeftToRight,
                        ReadingMode::RightToLeft,
                    ] {
                        if design::selectable_value(ui, &mut reader.mode, mode, mode_name(mode, l))
                            .changed()
                        {
                            reader.restore = true;
                            reader.dirty = true;
                            ui.close();
                        }
                    }
                });
                let display =
                    dropdown(ui, display_name(reader.display, l), None, bar.display_width);
                egui::Popup::menu(&display).show(|ui| {
                    for display in [Display::Original, Display::Translated, Display::Compare] {
                        if design::selectable_value(
                            ui,
                            &mut reader.display,
                            display,
                            display_name(display, l),
                        )
                        .changed()
                        {
                            reader.restore = true;
                            ui.close();
                        }
                    }
                });
                ui.add_enabled_ui(!reader.urls.is_empty(), |ui| {
                    if design::control(
                        ui,
                        l.text("翻译本页", "Translate page"),
                        Some(reader_translate),
                        design::Control::Outline,
                        0.0,
                    )
                    .clicked()
                    {
                        actions.translate = Some(false);
                    }
                    if design::control(
                        ui,
                        l.text("翻译本章", "Translate chapter"),
                        Some(reader_translate_chapter),
                        design::Control::Outline,
                        0.0,
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
            if design::control(ui, "", Some(reader_minus), design::Control::Quiet, 34.0)
                .on_hover_text(l.text("缩小", "Zoom out"))
                .clicked()
            {
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
            if design::control(ui, "", Some(reader_plus), design::Control::Quiet, 34.0)
                .on_hover_text(l.text("放大", "Zoom in"))
                .clicked()
            {
                reader.zoom = (reader.zoom + 0.1).min(2.0);
            }
        });
        if old_zoom != reader.zoom {
            reader.restore = true;
            reader.dirty = true;
        }
        let mut chapters = reader_slot(ui, "reader-chapters-toggle", bar.chapters);
        if design::control(
            &mut chapters,
            "",
            Some(reader_chapters),
            design::Control::Quiet,
            34.0,
        )
        .on_hover_text(l.text("展开 / 收起章节栏", "Toggle chapters"))
        .clicked()
        {
            reader.sidebar = !reader.sidebar;
            reader.sidebar_follow = true;
        }
        pager(ui, reader, bar.pager, neighbors, l, &mut actions);
    }
    if visible || status.working || !status.message.is_empty() || status.failed > 0 {
        let mut status_ui = reader_slot(ui, "reader-status", bar.status);
        let response = design::control(
            &mut status_ui,
            "",
            Some(info_icon),
            design::Control::Quiet,
            34.0,
        );
        if status.working || status.failed > 0 || !status.message.is_empty() {
            ui.painter().circle_filled(
                response.rect.right_top() + vec2(-6.0, 6.0),
                3.0,
                if status.failed > 0 || !status.message.is_empty() {
                    Color32::LIGHT_RED
                } else {
                    ACCENT
                },
            );
        }
        let response = response.on_hover_ui(|ui| {
            ui.set_max_width(320.0);
            status_text(ui, &status, l);
        });
        egui::Popup::menu(&response).show(|ui| {
            ui.set_max_width(320.0);
            status_text(ui, &status, l);
            if status.working
                && ui
                    .button(l.text("取消翻译", "Cancel translation"))
                    .clicked()
            {
                actions.cancel = true;
                ui.close();
            }
            if !reader.errors.is_empty()
                && ui
                    .button(l.text("重新加载本章图片", "Reload chapter images"))
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
    let cy = rect.center().y;
    let mut previous = reader_slot(
        ui,
        "reader-previous",
        Rect::from_min_size(rect.min, vec2(34.0, 34.0)),
    );
    previous.add_enabled_ui(neighbors[0].is_some(), |ui| {
        if design::control(ui, "", Some(draw_prev), design::Control::Quiet, 34.0)
            .on_hover_text(l.text("上一章", "Previous chapter"))
            .clicked()
        {
            actions.chapter = neighbors[0];
        }
    });
    let mut next = reader_slot(
        ui,
        "reader-next",
        Rect::from_min_size(pos2(rect.max.x - 34.0, rect.min.y), vec2(34.0, 34.0)),
    );
    next.add_enabled_ui(neighbors[1].is_some(), |ui| {
        if design::control(ui, "", Some(draw_next), design::Control::Quiet, 34.0)
            .on_hover_text(l.text("下一章", "Next chapter"))
            .clicked()
        {
            actions.chapter = neighbors[1];
        }
    });
    let count = reader.urls.len();
    ui.painter().text(
        pos2(rect.max.x - 74.0, cy),
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
        pos2(rect.min.x + 42.0, cy - 14.0),
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

fn dropdown(ui: &mut Ui, label: &str, icon: Option<design::Icon>, width: f32) -> egui::Response {
    let response = design::control(ui, label, icon, design::Control::Outline, width);
    let arrow = Rect::from_center_size(
        pos2(response.rect.max.x - 16.0, response.rect.center().y),
        vec2(16.0, 16.0),
    );
    icons::paint(ui.painter(), arrow, MUTED, icons::CARET_DOWN);
    response
}

fn display_name(display: Display, l: Language) -> &'static str {
    match display {
        Display::Original => l.text("原文", "Original"),
        Display::Translated => l.text("译文", "Translated"),
        Display::Compare => l.text("对照", "Compare"),
    }
}

fn status_text(ui: &mut Ui, status: &Status<'_>, l: Language) {
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
            ui.label(format!(
                "{} {}",
                status.failed,
                l.text("页失败", "pages failed")
            ));
        }
    } else if status.message.is_empty() {
        ui.label(l.text(
            "当前页优先 · 译图自动保存",
            "Current page first · Translations are saved automatically",
        ));
    }
    ui.small(l.text("全屏 / 退出全屏：F11", "Toggle fullscreen: F11"));
}

fn info_icon(p: &Painter, r: Rect, c: Color32) {
    icons::paint(p, r, c, icons::INFO);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_do_not_overlap_at_supported_window_sizes_and_offsets() {
        for width in [760.0, 960.0, 1100.0, 1280.0, 1920.0] {
            for options_width in [460.0, 620.0] {
                let screen = Rect::from_min_size(pos2(70.0, 35.0), vec2(width, 540.0));
                let bar = arrange(screen, options_width, 160.0, 110.0);
                assert_eq!(bar.footer.height(), 36.0);
                let controls = [bar.options, bar.pager, bar.zoom, bar.chapters, bar.status];
                for (index, rect) in controls.iter().enumerate() {
                    assert!(bar.footer.contains_rect(*rect));
                    for other in &controls[index + 1..] {
                        assert!(!rect.intersects(*other));
                    }
                }
                assert_eq!(
                    bar.pager.width() - 156.0,
                    if bar.compact { 120.0 } else { 180.0 }
                );
            }
        }
    }
}
