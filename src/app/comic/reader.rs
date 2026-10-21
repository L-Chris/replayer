use super::*;

impl State {
    pub(super) fn reader_ui(
        &mut self,
        ui: &mut Ui,
        settings: &crate::settings::Settings,
        settings_open: &mut bool,
    ) {
        let l = settings.language;
        let mut back = false;
        let mut chapter_action = None;
        let mut translate = None;
        let mut retry = false;
        let mut cancel_translation = false;
        let Some(reader) = &mut self.reader else {
            self.view = View::Detail;
            return;
        };
        let screen = ui.available_rect_before_wrap();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let near_edge = pointer.is_some_and(|p| {
            screen.contains(p) && (p.y < screen.min.y + 112.0 || p.y > screen.max.y - 78.0)
        });
        if near_edge || egui::Popup::is_any_open(ui.ctx()) || ui.ctx().dragged_id().is_some() {
            reader.toolbar_until = Instant::now() + Duration::from_millis(900);
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
            if fullscreen {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else if reader.sidebar {
                reader.sidebar = false;
            } else {
                back = true;
            }
        }
        if !ui.ctx().egui_wants_keyboard_input() && !*settings_open {
            if ui.input(|i| i.key_pressed(Key::F)) {
                let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            let delta = if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
                1
            } else if ui.input(|i| i.key_pressed(Key::ArrowLeft)) {
                -1
            } else {
                0
            };
            let delta = if reader.mode == ReadingMode::RightToLeft {
                -delta
            } else {
                delta
            };
            if delta != 0 {
                reader.current = (reader.current as i64 + delta)
                    .clamp(0, reader.urls.len().saturating_sub(1) as i64)
                    as usize;
                reader.fraction = 0.0;
                reader.restore = true;
                reader.dirty = true;
                reader.toolbar_until = Instant::now() + Duration::from_secs(2);
            }
        }
        let controls = Instant::now() < reader.toolbar_until;
        let compact = screen.width() < 1100.0;
        let header = Rect::from_min_size(screen.min, vec2(screen.width(), 108.0));
        let footer = Rect::from_min_max(pos2(screen.min.x, screen.max.y - 68.0), screen.max);
        let side_width = if reader.sidebar {
            (screen.width() * 0.21).clamp(200.0, 270.0)
        } else {
            0.0
        };
        let canvas_outer = Rect::from_min_max(
            screen.min + vec2(20.0, 116.0),
            pos2(
                screen.max.x - side_width - if reader.sidebar { 32.0 } else { 20.0 },
                footer.min.y - 10.0,
            ),
        );
        ui.painter().rect_filled(canvas_outer, 10, BACKGROUND);
        ui.painter().rect_stroke(
            canvas_outer,
            10,
            Stroke::new(1.0, design::LINE),
            egui::StrokeKind::Inside,
        );
        let labels = Rect::from_min_max(
            canvas_outer.min + vec2(16.0, 8.0),
            pos2(canvas_outer.max.x - 16.0, canvas_outer.min.y + 33.0),
        );
        let canvas = Rect::from_min_max(
            canvas_outer.min + vec2(14.0, 38.0),
            canvas_outer.max - vec2(14.0, 12.0),
        );
        if controls {
            ui.painter().rect_filled(header, 0, BACKGROUND);
            ui.painter().line_segment(
                [header.left_bottom(), header.right_bottom()],
                Stroke::new(1.0, design::LINE),
            );
            let caption = Rect::from_min_max(
                screen.min + vec2(22.0, 8.0),
                pos2(
                    screen.max.x - window::CONTROLS_WIDTH - 12.0,
                    screen.min.y + 44.0,
                ),
            );
            let mut caption_ui = workspace::child(ui, "reader-caption", caption);
            caption_ui.horizontal(|ui| {
                let (mark, _) = ui.allocate_exact_size(vec2(23.0, 26.0), Sense::hover());
                draw_play(ui.painter(), mark.shrink(3.0), ACCENT);
                if !compact {
                    ui.label(RichText::new("replayer").size(20.0).strong());
                    ui.separator();
                }
                if icon_button(ui, 26.0, 1.0, reader_back)
                    .on_hover_text(l.text("返回目录", "Back to chapters"))
                    .clicked()
                {
                    back = true;
                }
                ui.add(egui::Label::new(RichText::new(&reader.book.title).strong()).truncate());
                ui.label(RichText::new("›").color(MUTED));
                ui.add(
                    egui::Label::new(RichText::new(&reader.chapter.name).color(MUTED)).truncate(),
                );
            });
            let mut toolbar = workspace::child(
                ui,
                "reader-toolbar",
                Rect::from_min_max(
                    screen.min + vec2(22.0, 54.0),
                    pos2(screen.max.x - 22.0, screen.min.y + 94.0),
                ),
            );
            toolbar.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if design::control(ui, "", Some(reader_chapters), design::Control::Quiet, 34.0)
                    .on_hover_text(l.text("展开 / 收起章节栏", "Toggle chapters"))
                    .clicked()
                {
                    reader.sidebar = !reader.sidebar;
                    reader.sidebar_follow = true;
                }
                if self.translation_total > 0 {
                    if self.translation.is_some() {
                        ui.spinner().on_hover_text(format!(
                            "{}/{}",
                            self.translation_done, self.translation_total
                        ));
                    }
                    if !compact {
                        ui.label(
                            RichText::new(format!(
                                "{} {}/{}",
                                l.text("翻译中", "Translating"),
                                self.translation_done,
                                self.translation_total
                            ))
                            .size(12.0)
                            .color(MUTED),
                        );
                    }
                }
                ui.add_enabled_ui(!reader.urls.is_empty(), |ui| {
                    if compact {
                        let response = design::control(
                            ui,
                            l.text("图片翻译", "Translate"),
                            Some(reader_translate),
                            design::Control::Outline,
                            0.0,
                        );
                        egui::Popup::menu(&response).show(|ui| {
                            if ui.button(l.text("翻译本页", "Translate page")).clicked() {
                                translate = Some(false);
                                ui.close();
                            }
                            if ui.button(l.text("翻译本章", "Translate chapter")).clicked() {
                                translate = Some(true);
                                ui.close();
                            }
                        });
                    } else {
                        if design::control(
                            ui,
                            l.text("翻译本章", "Translate chapter"),
                            Some(reader_translate_chapter),
                            design::Control::Outline,
                            0.0,
                        )
                        .clicked()
                        {
                            translate = Some(true);
                        }
                        if design::control(
                            ui,
                            l.text("翻译本页", "Translate page"),
                            Some(reader_translate),
                            design::Control::Outline,
                            0.0,
                        )
                        .clicked()
                        {
                            translate = Some(false);
                        }
                    }
                });
                let mut display = match reader.display {
                    Display::Original => 0,
                    Display::Translated => 1,
                    Display::Compare => 2,
                };
                if design::segmented(
                    ui,
                    &mut display,
                    &[
                        l.text("原文", "Original"),
                        l.text("译文", "Translated"),
                        l.text("对照", "Compare"),
                    ],
                ) {
                    reader.display = match display {
                        0 => Display::Original,
                        1 => Display::Translated,
                        _ => Display::Compare,
                    };
                    reader.restore = true;
                }
                let mode = design::control(
                    ui,
                    mode_name(reader.mode, l),
                    Some(reader_book),
                    design::Control::Outline,
                    if compact { 160.0 } else { 184.0 },
                );
                let arrow = Rect::from_center_size(
                    pos2(mode.rect.max.x - 17.0, mode.rect.center().y),
                    vec2(10.0, 6.0),
                );
                ui.painter().add(Shape::line(
                    vec![arrow.left_top(), arrow.center_bottom(), arrow.right_top()],
                    Stroke::new(1.4, MUTED),
                ));
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
            });
        }

        let target = match settings.comics.target.as_str() {
            "zh-CN" => "中文译图",
            "en-US" => "English translation",
            _ => l.text("中文译图", "English translation"),
        };
        ui.painter().text(
            labels.left_center(),
            egui::Align2::LEFT_CENTER,
            if reader.display == Display::Translated {
                target
            } else {
                l.text("原文", "Original")
            },
            egui::FontId::proportional(14.0),
            design::TEXT,
        );
        if reader.display == Display::Compare {
            ui.painter().text(
                pos2(labels.center().x + 6.0, labels.center().y),
                egui::Align2::LEFT_CENTER,
                target,
                egui::FontId::proportional(14.0),
                design::TEXT,
            );
        }
        let mut canvas_ui = workspace::child(ui, "comic_canvas", canvas);
        if reader.pages_job.is_some() {
            canvas_ui.horizontal(|ui| {
                ui.spinner();
                ui.label(l.text("正在加载章节…", "Loading chapter…"));
            });
        }
        if let Some(error) = &reader.pages_error {
            canvas_ui.colored_label(Color32::LIGHT_RED, error);
            if canvas_ui
                .button(l.text("重试章节", "Retry chapter"))
                .clicked()
            {
                retry = true;
            }
        }
        let count = reader.urls.len();
        let mut requested = vec![reader.current];
        if count > 0 {
            if reader.mode == ReadingMode::Vertical {
                let width = (canvas.width() - ui.spacing().scroll.allocated_width() - 4.0)
                    .max(40.0)
                    * reader.zoom;
                let compare = reader.display == Display::Compare;
                let page_width = if compare { (width - 10.0) / 2.0 } else { width };
                let height_for = |index: usize| {
                    let (w, h) = reader.sizes.get(&index).copied().unwrap_or((800.0, 1120.0));
                    page_width * h / w
                };
                let offset = (0..reader.current)
                    .map(|i| height_for(i) + 10.0)
                    .sum::<f32>()
                    + height_for(reader.current) * reader.fraction;
                let mut scroll = egui::ScrollArea::both().id_salt((
                    "comic_vertical",
                    reader.book.key(),
                    reader.chapter.key(),
                ));
                if reader.restore {
                    scroll = scroll.vertical_scroll_offset(offset);
                    reader.restore = false;
                }
                let output = scroll.show(&mut canvas_ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    let viewport = ui.clip_rect();
                    let mut closest = (f32::MAX, reader.current, reader.fraction);
                    for index in 0..count {
                        let height = height_for(index);
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(width, height), Sense::click());
                        if !viewport.intersects(rect) {
                            continue;
                        }
                        requested.push(index);
                        let top = (viewport.min.y - rect.min.y).max(0.0);
                        let distance = (rect.min.y - viewport.min.y).abs();
                        let distance = if rect.contains(pos2(rect.center().x, viewport.min.y + 2.0))
                        {
                            0.0
                        } else {
                            distance
                        };
                        if distance < closest.0 {
                            closest = (distance, index, (top / height).clamp(0.0, 1.0));
                        }
                        paint_page(ui, rect, reader.assets.get(&index), reader.display, l);
                        if let Some(error) = reader.errors.get(&index) {
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                error,
                                egui::FontId::proportional(13.0),
                                Color32::LIGHT_RED,
                            );
                        }
                        if response.double_clicked() {
                            reader.zoom = if reader.zoom > 1.1 { 1.0 } else { 1.5 };
                            reader.restore = true;
                        }
                        if index == count - 1
                            && rect.max.y <= viewport.max.y + 2.0
                            && reader.assets.contains_key(&index)
                        {
                            reader.completed = true;
                            reader.dirty = true;
                        }
                    }
                    closest
                });
                let (_, index, fraction) = output.inner;
                if reader.current != index || (reader.fraction - fraction).abs() > 0.005 {
                    reader.current = index;
                    reader.fraction = fraction;
                    reader.dirty = true;
                }
            } else {
                let current = reader.current;
                let size = reader
                    .sizes
                    .get(&current)
                    .copied()
                    .unwrap_or((800.0, 1120.0));
                let compare = reader.display == Display::Compare;
                egui::ScrollArea::both()
                    .id_salt(("comic_paged", reader.chapter.key(), current))
                    .auto_shrink([false, false])
                    .show_viewport(&mut canvas_ui, |ui, viewport| {
                        let available = if compare {
                            (viewport.width() - 18.0) / 2.0
                        } else {
                            viewport.width() - 8.0
                        };
                        let scale = (available / size.0).min((viewport.height() - 8.0) / size.1)
                            * reader.zoom;
                        let width = size.0 * scale;
                        let height = size.1 * scale;
                        ui.add_space(((viewport.height() - height) * 0.5).max(0.0));
                        ui.horizontal(|ui| {
                            let display_width = if compare { width * 2.0 + 10.0 } else { width };
                            ui.add_space(((viewport.width() - display_width) * 0.5).max(0.0));
                            let (rect, response) = ui.allocate_exact_size(
                                vec2(if compare { width * 2.0 + 10.0 } else { width }, height),
                                Sense::click(),
                            );
                            paint_page(ui, rect, reader.assets.get(&current), reader.display, l);
                            if response.clicked() {
                                let pointer =
                                    response.interact_pointer_pos().unwrap_or(rect.center());
                                let next = pointer.x > rect.center().x;
                                let next = if reader.mode == ReadingMode::RightToLeft {
                                    !next
                                } else {
                                    next
                                };
                                reader.current = if next {
                                    (current + 1).min(count - 1)
                                } else {
                                    current.saturating_sub(1)
                                };
                                reader.dirty = true;
                            }
                            if current == count - 1 && reader.assets.contains_key(&current) {
                                reader.completed = true;
                                reader.dirty = true;
                            }
                        });
                    });
            }
        }
        if reader.sidebar {
            let rect = Rect::from_min_max(
                pos2(screen.max.x - side_width - 20.0, canvas_outer.min.y),
                pos2(screen.max.x - 20.0, canvas_outer.max.y),
            );
            ui.painter().rect_filled(rect, 10, SURFACE);
            ui.painter().rect_stroke(
                rect,
                10,
                Stroke::new(1.0, design::LINE),
                egui::StrokeKind::Inside,
            );
            let mut sidebar = workspace::child(ui, "reader-chapter-panel", rect.shrink(14.0));
            sidebar.horizontal(|ui| {
                ui.label(
                    RichText::new(l.text("章节目录", "Chapters"))
                        .size(19.0)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, 24.0, 1.0, reader_chapters)
                        .on_hover_text(l.text("收起章节栏", "Collapse chapters"))
                        .clicked()
                    {
                        reader.sidebar = false;
                    }
                });
            });
            sidebar.add_space(8.0);
            let entry = self
                .library
                .as_ref()
                .and_then(|lib| lib.get(&reader.book.key()))
                .cloned();
            if let Some(entry) = entry {
                let mut scroll = design::scroll()
                    .id_salt("reader_chapters")
                    .auto_shrink([false, false]);
                if reader.sidebar_follow {
                    scroll = scroll.vertical_scroll_offset(
                        (reader.index as f32 * 80.0 - rect.height() * 0.4).max(0.0),
                    );
                }
                scroll.show_rows(&mut sidebar, 70.0, entry.chapters.len(), |ui, rows| {
                    for index in rows {
                        let chapter = &entry.chapters[index];
                        let current = index == reader.index;
                        let (row, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 70.0), Sense::click());
                        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if current || response.hovered() {
                            ui.painter().rect_filled(
                                row,
                                8,
                                if current { ACCENT } else { BACKGROUND },
                            );
                        }
                        let art = Rect::from_min_size(row.min + vec2(8.0, 9.0), vec2(38.0, 52.0));
                        if let Some(cover) = self.covers.get(&reader.book.key()) {
                            cover.paint(ui, art);
                        } else {
                            ui.painter().rect_filled(art, 4, BACKGROUND);
                            ui.painter().text(
                                art.center(),
                                egui::Align2::CENTER_CENTER,
                                (index + 1).to_string(),
                                egui::FontId::proportional(14.0),
                                MUTED,
                            );
                        }
                        let mut text = workspace::child(
                            ui,
                            ("chapter", index),
                            Rect::from_min_max(
                                row.min + vec2(56.0, 12.0),
                                row.max - vec2(8.0, 6.0),
                            ),
                        );
                        text.style_mut().interaction.selectable_labels = false;
                        text.add(
                            egui::Label::new(RichText::new(&chapter.name).color(if current {
                                BACKGROUND
                            } else {
                                design::TEXT
                            }))
                            .truncate(),
                        );
                        text.label(
                            RichText::new(if current {
                                l.text("正在阅读", "Reading")
                            } else if entry.read.contains(chapter.key()) {
                                l.text("已读", "Read")
                            } else if entry.new_unread.contains(chapter.key()) {
                                l.text("新章节", "New chapter")
                            } else {
                                l.text("未读", "Unread")
                            })
                            .small()
                            .color(if current {
                                BACKGROUND.gamma_multiply(0.75)
                            } else {
                                MUTED
                            }),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &chapter.name)
                        });
                        if current && reader.sidebar_follow {
                            ui.scroll_to_rect(row, Some(Align::Center));
                        }
                        if response.on_hover_text(&chapter.name).clicked() {
                            chapter_action = Some(index);
                        }
                    }
                });
            }
        }
        reader.sidebar_follow = false;
        if controls {
            ui.painter().rect_filled(footer, 0, BACKGROUND);
            ui.painter().line_segment(
                [footer.left_top(), footer.right_top()],
                Stroke::new(1.0, design::LINE),
            );
            let cy = footer.center().y;
            let status_width = if compact { 112.0 } else { 260.0 };
            let status = if compact {
                l.text("状态", "Status")
            } else if !self.status.is_empty() {
                l.text("操作失败 · 查看详情", "Failed · Details")
            } else if self.translation_total > 0 {
                l.text("图片翻译 · 查看进度", "Translation · Progress")
            } else {
                l.text(
                    "当前页优先 · 译图自动保存",
                    "Current page first · Auto-saved",
                )
            };
            let mut status_ui = reader_slot(
                ui,
                "reader-status",
                Rect::from_min_size(
                    pos2(footer.min.x + 16.0, cy - 17.0),
                    vec2(status_width - 20.0, 34.0),
                ),
            );
            status_ui
                .style_mut()
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(12.0));
            let status_response = design::control(
                &mut status_ui,
                status,
                Some(reader_document),
                design::Control::Quiet,
                status_width - 20.0,
            );
            egui::Popup::menu(&status_response).show(|ui| {
                ui.set_max_width(360.0);
                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
                if self.translation_total > 0 {
                    ui.label(format!(
                        "{} {}/{} · {} {}",
                        l.text("图片翻译", "Translation"),
                        self.translation_done,
                        self.translation_total,
                        self.translation_failed,
                        l.text("页失败", "pages failed")
                    ));
                }
                if (self.translation.is_some() || !self.translations.is_empty())
                    && ui
                        .button(l.text("取消翻译", "Cancel translation"))
                        .clicked()
                {
                    cancel_translation = true;
                    ui.close();
                }
                if !reader.errors.is_empty()
                    && ui
                        .button(l.text("重新加载本章图片", "Reload chapter images"))
                        .clicked()
                {
                    retry = true;
                    ui.close();
                }
            });

            let mut x = footer.max.x - 20.0;
            let mut fullscreen = reader_slot(
                ui,
                "reader-fullscreen",
                Rect::from_center_size(pos2(x - 16.0, cy), vec2(32.0, 34.0)),
            );
            if design::control(
                &mut fullscreen,
                "",
                Some(draw_fullscreen),
                design::Control::Quiet,
                32.0,
            )
            .on_hover_text(l.text("全屏 · F", "Fullscreen · F"))
            .clicked()
            {
                let full = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
            }
            x -= 52.0;
            let mut zoom_in = reader_slot(
                ui,
                "reader-zoom-in",
                Rect::from_center_size(pos2(x - 17.0, cy), vec2(34.0, 34.0)),
            );
            let mut zoom_changed = false;
            if design::control(
                &mut zoom_in,
                "",
                Some(reader_plus),
                design::Control::Circle,
                34.0,
            )
            .on_hover_text(l.text("放大", "Zoom in"))
            .clicked()
            {
                reader.zoom = (reader.zoom + 0.1).min(2.0);
                zoom_changed = true;
            }
            x -= 42.0;
            let mut zoom_value = reader_slot(
                ui,
                "reader-zoom-value",
                Rect::from_center_size(pos2(x - 28.0, cy), vec2(56.0, 34.0)),
            );
            if design::control(
                &mut zoom_value,
                &format!("{:.0}%", reader.zoom * 100.0),
                None,
                design::Control::Quiet,
                56.0,
            )
            .on_hover_text(l.text("恢复 100%", "Reset zoom"))
            .clicked()
            {
                reader.zoom = 1.0;
                zoom_changed = true;
            }
            x -= 64.0;
            let mut zoom_out = reader_slot(
                ui,
                "reader-zoom-out",
                Rect::from_center_size(pos2(x - 17.0, cy), vec2(34.0, 34.0)),
            );
            if design::control(
                &mut zoom_out,
                "",
                Some(reader_minus),
                design::Control::Circle,
                34.0,
            )
            .on_hover_text(l.text("缩小", "Zoom out"))
            .clicked()
            {
                reader.zoom = (reader.zoom - 0.1).max(0.5);
                zoom_changed = true;
            }
            if zoom_changed {
                reader.restore = true;
                reader.dirty = true;
            }
            x -= 50.0;
            let pager = Rect::from_min_max(
                pos2(footer.min.x + status_width + 16.0, cy - 17.0),
                pos2(x, cy + 17.0),
            );
            let button_width = 34.0;
            let total = self
                .library
                .as_ref()
                .and_then(|lib| lib.get(&reader.book.key()))
                .map_or(0, |e| e.chapters.len());
            let mut previous = reader_slot(
                ui,
                "reader-previous",
                Rect::from_min_size(pager.min, vec2(button_width, 34.0)),
            );
            previous.add_enabled_ui(reader.index > 0, |ui| {
                if design::control(
                    ui,
                    "",
                    Some(draw_prev),
                    design::Control::Quiet,
                    button_width,
                )
                .on_hover_text(l.text("上一章", "Previous chapter"))
                .clicked()
                {
                    chapter_action = Some(reader.index - 1);
                }
            });
            let mut next = reader_slot(
                ui,
                "reader-next",
                Rect::from_min_size(
                    pos2(pager.max.x - button_width, cy - 17.0),
                    vec2(button_width, 34.0),
                ),
            );
            next.add_enabled_ui(reader.index + 1 < total, |ui| {
                let response = design::control(
                    ui,
                    "",
                    Some(draw_next),
                    design::Control::Quiet,
                    button_width,
                );
                if response
                    .on_hover_text(l.text("下一章", "Next chapter"))
                    .clicked()
                {
                    chapter_action = Some(reader.index + 1);
                }
            });
            let count_width = if compact { 48.0 } else { 64.0 };
            let count_right = pager.max.x - button_width - 10.0;
            ui.painter().text(
                pos2(count_right - count_width * 0.5, cy),
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
                pos2(pager.min.x + button_width + 10.0, cy - 14.0),
                pos2(count_right - count_width - 10.0, cy + 14.0),
            );
            let mut track_ui = reader_slot(ui, "reader-page-track", track_rect);
            let fraction = if count > 1 {
                reader.current as f32 / (count - 1) as f32
            } else {
                0.0
            };
            let (track, response) = hslider(
                &mut track_ui,
                track_rect.width().max(20.0),
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
        let requested = page_requests(reader.current, count, requested);
        // Free all obsolete slots before scheduling the new visible page.
        // Otherwise a jump can wait for slow prefetches around the old page.
        reader.jobs.retain(|index, _| requested.contains(index));
        if let Some(position) = self.translations.iter().position(|r| {
            r.book == reader.book.key()
                && r.chapter == reader.chapter.key()
                && r.index == reader.current
        }) && let Some(request) = self.translations.remove(position)
        {
            self.translations.push_front(request);
        }
        if cancel_translation {
            self.translation = None;
            self.translations.clear();
            self.translation_total = self.translation_done;
        }
        if back {
            self.flush();
            self.reader = None;
            self.view = View::Detail;
            self.translations.clear();
            return;
        }
        if retry
            && let Some(reader) = &mut self.reader
            && let Some(source) = self.source.clone()
        {
            let book = reader.book.clone();
            let chapter = reader.chapter.clone();
            reader.jobs.clear();
            reader.errors.clear();
            reader.pages_error = None;
            reader.assets.clear();
            for index in 0..reader.urls.len() {
                let key = format!("{}:{}:{index}", book.key(), chapter.key());
                let _ = std::fs::remove_file(
                    domain::root()
                        .join("cache/pages")
                        .join(format!("{}.img", domain::digest(key.as_bytes()))),
                );
            }
            reader.pages_job = Some(Job::spawn(move |_| async move {
                source.pages(&book, &chapter, true).await
            }));
        }
        if let Some(index) = chapter_action {
            self.read_chapter(index, settings, false);
            return;
        }
        for index in requested {
            self.ensure_page(index, settings);
        }
        if let Some(all) = translate {
            self.queue_translation(all, settings);
        }
        ui.allocate_space(vec2(ui.available_width(), (screen.height() - 8.0).max(0.0)));
    }
}
fn mode_name(mode: ReadingMode, l: Language) -> &'static str {
    match mode {
        ReadingMode::Vertical => l.text("条漫滚动", "Vertical scroll"),
        ReadingMode::LeftToRight => l.text("从左向右", "Left to right"),
        ReadingMode::RightToLeft => l.text("从右向左", "Right to left"),
    }
}
fn paint_page(ui: &Ui, rect: Rect, asset: Option<&Asset>, display: Display, l: Language) {
    ui.painter().rect_filled(rect, 0, BACKGROUND);
    let Some(asset) = asset else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            l.text("正在加载图片…", "Loading image…"),
            egui::FontId::proportional(15.0),
            MUTED,
        );
        return;
    };
    match display {
        Display::Original => asset.original.paint(ui, rect),
        Display::Translated => {
            if let Some(translated) = &asset.translated {
                translated.paint(ui, rect);
            } else {
                paint_translation_pending(ui, rect, l);
            }
        }
        Display::Compare => {
            let width = (rect.width() - 10.0) / 2.0;
            let left = Rect::from_min_size(rect.min, vec2(width, rect.height()));
            let right = Rect::from_min_size(pos2(left.max.x + 10.0, rect.min.y), left.size());
            asset.original.paint(ui, left);
            if let Some(translated) = &asset.translated {
                translated.paint(ui, right);
            } else {
                paint_translation_pending(ui, right, l);
            }
        }
    }
}
fn paint_translation_pending(ui: &Ui, rect: Rect, l: Language) {
    ui.painter().rect_filled(rect, 6, SURFACE);
    ui.painter().text(
        rect.center() - vec2(0.0, 15.0),
        egui::Align2::CENTER_CENTER,
        l.text("本页尚未翻译", "This page has not been translated"),
        egui::FontId::proportional(18.0),
        design::TEXT,
    );
    ui.painter().text(
        rect.center() + vec2(0.0, 17.0),
        egui::Align2::CENTER_CENTER,
        l.text(
            "使用上方「翻译本页」生成译图",
            "Use Translate page to generate an edited image",
        ),
        egui::FontId::proportional(12.0),
        MUTED,
    );
}
fn reader_slot(ui: &mut Ui, id: &str, rect: Rect) -> Ui {
    workspace::child(ui, id, rect)
}

fn reader_plus(p: &Painter, r: Rect, c: Color32) {
    p.line_segment([r.left_center(), r.right_center()], Stroke::new(1.5, c));
    p.line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.5, c));
}
fn reader_minus(p: &Painter, r: Rect, c: Color32) {
    p.line_segment([r.left_center(), r.right_center()], Stroke::new(1.5, c));
}
fn reader_chapters(p: &Painter, r: Rect, c: Color32) {
    for y in [0.0, 5.0] {
        p.add(Shape::line(
            vec![
                pos2(r.min.x, r.center().y + y),
                pos2(r.center().x, r.min.y + y),
                pos2(r.max.x, r.center().y + y),
            ],
            Stroke::new(1.4, c),
        ));
    }
}
pub(super) fn reader_book(p: &Painter, r: Rect, c: Color32) {
    p.rect_stroke(r, 2, Stroke::new(1.3, c), egui::StrokeKind::Inside);
    p.line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.3, c));
}
fn reader_document(p: &Painter, r: Rect, c: Color32) {
    p.rect_stroke(
        r.shrink2(vec2(2.0, 0.0)),
        2,
        Stroke::new(1.3, c),
        egui::StrokeKind::Inside,
    );
    for y in [0.4, 0.65] {
        p.line_segment(
            [
                pos2(r.min.x + 5.0, r.min.y + r.height() * y),
                pos2(r.max.x - 5.0, r.min.y + r.height() * y),
            ],
            Stroke::new(1.2, c),
        );
    }
}
fn reader_translate(p: &Painter, r: Rect, c: Color32) {
    reader_document(p, r, c);
    p.circle_filled(r.right_top() + vec2(-1.0, 1.0), 2.5, c);
}
fn reader_translate_chapter(p: &Painter, r: Rect, c: Color32) {
    reader_document(p, r.translate(vec2(2.0, 1.0)).shrink(1.0), c);
    p.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.2, c));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.2, c));
}
fn reader_back(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.6, c);
    let center = r.center();
    p.line_segment([pos2(r.min.x, center.y), pos2(r.max.x, center.y)], s);
    p.line_segment(
        [
            pos2(r.min.x, center.y),
            center + vec2(0.0, -r.height() * 0.4),
        ],
        s,
    );
    p.line_segment(
        [
            pos2(r.min.x, center.y),
            center + vec2(0.0, r.height() * 0.4),
        ],
        s,
    );
}
