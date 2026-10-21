use super::*;

impl State {
    pub(super) fn catalog(&mut self, ui: &mut Ui, settings: &crate::settings::Settings) {
        let l = settings.language;
        let updates = self
            .entries()
            .iter()
            .filter(|e| e.favorite && !e.new_unread.is_empty())
            .count();
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Shelf, l.text("书架", "Library"));
            ui.selectable_value(&mut self.tab, Tab::Search, l.text("搜索", "Search"));
            ui.selectable_value(
                &mut self.tab,
                Tab::Updates,
                format!("{} · {updates}", l.text("更新", "Updates")),
            );
            ui.selectable_value(&mut self.tab, Tab::History, l.text("历史", "History"));
            if ui
                .add_enabled(
                    self.update_job.is_none() && self.update_queue.is_empty(),
                    egui::Button::new(l.text("检查更新", "Check updates")),
                )
                .clicked()
            {
                self.begin_updates(true);
            }
        });
        if self.update_total > 0 {
            ui.label(format!(
                "{} {}/{}",
                l.text("更新检查", "Checking"),
                self.update_done,
                self.update_total
            ));
        }
        if self.tab == Tab::Search {
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.keyword)
                        .hint_text(l.text("漫画名称", "Comic title"))
                        .desired_width((ui.available_width() - 170.0).max(80.0)),
                );
                let submit = ui.button(l.text("搜索", "Search")).clicked()
                    || (field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                if submit
                    && !self.keyword.trim().is_empty()
                    && let Some(source) = self.source.clone()
                {
                    let keyword = self.keyword.trim().to_owned();
                    self.groups.clear();
                    self.status.clear();
                    self.search =
                        Some(Job::spawn(
                            move |_| async move { source.search(&keyword).await },
                        ));
                }
                if self.search.is_some() {
                    ui.spinner();
                    if ui.small_button(l.text("取消", "Cancel")).clicked() {
                        self.search = None;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(l.text("来源", "Source"));
                let selected = self
                    .groups
                    .iter()
                    .find(|g| g.site.id == self.site_filter)
                    .map(|g| g.site.name.as_str())
                    .unwrap_or(l.text("全部来源", "All sources"));
                egui::ComboBox::from_id_salt("comic_source")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.site_filter,
                            String::new(),
                            l.text("全部来源", "All sources"),
                        );
                        for group in &self.groups {
                            ui.selectable_value(
                                &mut self.site_filter,
                                group.site.id.clone(),
                                &group.site.name,
                            );
                        }
                    });
            });
            egui::ScrollArea::vertical()
                .id_salt("comic_search_results")
                .max_height((ui.available_height() - 70.0).max(100.0))
                .show(ui, |ui| {
                    let groups: Vec<_> = self
                        .groups
                        .iter()
                        .filter(|g| self.site_filter.is_empty() || g.site.id == self.site_filter)
                        .cloned()
                        .collect();
                    if groups.is_empty() && self.search.is_none() {
                        ui.label(l.text("输入名称搜索漫画", "Search for a comic by title"));
                    }
                    for group in groups {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&group.site.name).strong());
                            ui.small(format!("{}", group.books.len()));
                            if let Some(error) = &group.error {
                                ui.colored_label(Color32::LIGHT_RED, error);
                            } else if group.books.is_empty() {
                                ui.weak(l.text("没有结果", "No results"));
                            }
                        });
                        self.book_grid(ui, &group.books, settings);
                        ui.add_space(12.0);
                    }
                });
            if !self.sites.is_empty() {
                ui.weak(format!(
                    "{} {}",
                    self.sites.len(),
                    l.text("个漫画来源", "comic sources")
                ));
            }
        } else {
            egui::ScrollArea::vertical()
                .id_salt(("comic_library", self.tab as u8))
                .max_height((ui.available_height() - 65.0).max(100.0))
                .show(ui, |ui| {
                    if self.tab == Tab::Shelf
                        && let Some(entry) = self
                            .entries()
                            .iter()
                            .filter(|e| e.progress.is_some() && e.last_read > 0)
                            .max_by_key(|e| e.last_read)
                            .cloned()
                    {
                        egui::Frame::new()
                            .fill(SURFACE)
                            .corner_radius(12)
                            .inner_margin(16)
                            .show(ui, |ui| {
                                ui.set_min_width((ui.available_width() - 32.0).max(130.0));
                                ui.label(
                                    RichText::new(l.text("继续阅读", "Continue reading"))
                                        .color(ACCENT),
                                );
                                ui.label(RichText::new(&entry.book.title).size(20.0).strong());
                                if let Some(progress) = &entry.progress {
                                    let chapter = entry
                                        .chapters
                                        .iter()
                                        .find(|c| c.key() == progress.chapter)
                                        .map(|c| c.name.as_str())
                                        .unwrap_or("");
                                    ui.label(format!(
                                        "{chapter} · {} {}",
                                        l.text("页", "Page"),
                                        progress.page + 1
                                    ));
                                }
                                if ui.button(l.text("继续阅读", "Continue reading")).clicked() {
                                    self.resume(entry, settings);
                                }
                            });
                        ui.add_space(16.0);
                    }
                    let mut entries: Vec<_> = self
                        .entries()
                        .iter()
                        .filter(|e| match self.tab {
                            Tab::Shelf => e.favorite,
                            Tab::History => e.last_read > 0,
                            Tab::Updates => {
                                e.favorite && (!e.new_unread.is_empty() || e.check_error.is_some())
                            }
                            Tab::Search => false,
                        })
                        .cloned()
                        .collect();
                    entries.sort_by_key(|e| {
                        std::cmp::Reverse(if self.tab == Tab::History {
                            e.last_read
                        } else {
                            e.checked.max(e.last_read)
                        })
                    });
                    if entries.is_empty() {
                        ui.add_space(20.0);
                        ui.label(l.text(
                            "暂无漫画，先搜索并收藏你喜欢的作品",
                            "No comics here yet. Search and add a favorite.",
                        ));
                        if ui.button(l.text("搜索漫画", "Search comics")).clicked() {
                            self.tab = Tab::Search;
                        }
                    }
                    if self.tab == Tab::Updates {
                        for entry in &entries {
                            if let Some(error) = &entry.check_error {
                                ui.colored_label(
                                    Color32::LIGHT_RED,
                                    format!("{} · {error}", entry.book.title),
                                );
                            }
                        }
                    }
                    self.book_grid(
                        ui,
                        &entries.into_iter().map(|e| e.book).collect::<Vec<_>>(),
                        settings,
                    );
                });
        }
    }
    pub(super) fn book_grid(
        &mut self,
        ui: &mut Ui,
        books: &[Book],
        settings: &crate::settings::Settings,
    ) -> bool {
        let l = settings.language;
        let columns = (ui.available_width() / 170.0).floor().max(1.0) as usize;
        let width = ((ui.available_width() - 12.0 * (columns.saturating_sub(1)) as f32)
            / columns as f32)
            .clamp(100.0, 180.0);
        let mut open = None;
        for row in books.chunks(columns) {
            ui.horizontal_top(|ui| {
                for book in row {
                    ui.push_id(book.key(), |ui| {
                        ui.allocate_ui_with_layout(
                            vec2(width, width * 1.48 + 73.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(width);
                                ui.spacing_mut().item_spacing.y = 4.0;
                                let (rect, response) = ui
                                    .allocate_exact_size(vec2(width, width * 1.5), Sense::click());
                                ui.painter().rect_filled(rect, 8, SURFACE);
                                if let Some(picture) = self.covers.get(&book.key()) {
                                    picture.paint(ui, rect);
                                } else {
                                    let outline =
                                        Rect::from_center_size(rect.center(), vec2(28.0, 34.0));
                                    ui.painter().rect_stroke(
                                        outline,
                                        3,
                                        Stroke::new(1.5, MUTED),
                                        egui::StrokeKind::Inside,
                                    );
                                    ui.painter().line_segment(
                                        [
                                            outline.min + vec2(7.0, 0.0),
                                            pos2(outline.min.x + 7.0, outline.max.y),
                                        ],
                                        Stroke::new(1.5, MUTED),
                                    );
                                }
                                if ui.clip_rect().intersects(rect) {
                                    self.ensure_cover(book);
                                }
                                let entry =
                                    self.library.as_ref().and_then(|lib| lib.get(&book.key()));
                                if let Some(entry) = entry
                                    && entry.favorite
                                    && !entry.new_unread.is_empty()
                                {
                                    let center = pos2(rect.max.x - 9.0, rect.min.y + 9.0);
                                    ui.painter().circle_filled(
                                        center,
                                        5.0,
                                        Color32::from_rgb(246, 91, 109),
                                    );
                                }
                                if response.clicked() {
                                    open = Some(book.clone());
                                }
                                let label = ui
                                    .add(
                                        egui::Label::new(RichText::new(&book.title).strong())
                                            .truncate()
                                            .sense(Sense::click()),
                                    )
                                    .on_hover_text(&book.title);
                                if label.clicked() {
                                    open = Some(book.clone());
                                }
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&book.site_name).small().color(MUTED),
                                    )
                                    .truncate(),
                                );
                                if let Some(entry) = entry {
                                    if !entry.new_unread.is_empty() {
                                        ui.small(format!(
                                            "{} {}",
                                            entry.new_unread.len(),
                                            l.text("新章节未读", "new unread chapters")
                                        ));
                                    } else if let Some(progress) = &entry.progress {
                                        ui.small(format!(
                                            "{} {}",
                                            l.text("读到第", "Page"),
                                            progress.page + 1
                                        ));
                                    }
                                }
                            },
                        );
                    });
                }
            });
        }
        if let Some(book) = open {
            self.open_book(book);
            true
        } else {
            false
        }
    }
    pub(super) fn detail_ui(&mut self, ui: &mut Ui, settings: &crate::settings::Settings) {
        let l = settings.language;
        if ui
            .button(l.text("← 返回书架 / 搜索", "← Back to library / search"))
            .clicked()
        {
            self.view = View::Catalog;
            self.detail_job = None;
            return;
        }
        let Some(book) = self.selected.clone() else {
            return;
        };
        let entry = self.selected_entry();
        let shown = entry.as_ref().map(|e| &e.book).unwrap_or(&book).clone();
        ui.horizontal_top(|ui| {
            let width = (ui.available_width() * 0.22).clamp(80.0, 190.0);
            let (rect, _) = ui.allocate_exact_size(vec2(width, width * 1.5), Sense::hover());
            ui.painter().rect_filled(rect, 8, SURFACE);
            if let Some(picture) = self.covers.get(&shown.key()) {
                picture.paint(ui, rect);
            }
            self.ensure_cover(&shown);
            ui.vertical(|ui| {
                ui.label(RichText::new(&shown.title).size(29.0).strong());
                ui.label(format!("{} · {}", shown.author, shown.site_name));
                if !shown.status.is_empty() {
                    ui.small(&shown.status);
                }
                ui.horizontal_wrapped(|ui| {
                    if let Some(entry) = &entry {
                        if ui
                            .button(if entry.favorite {
                                l.text("已收藏 ★", "Favorited ★")
                            } else {
                                l.text("收藏 ☆", "Favorite ☆")
                            })
                            .clicked()
                        {
                            let mut entry = entry.clone();
                            entry.set_favorite(!entry.favorite);
                            self.save(entry);
                        }
                        if ui
                            .add_enabled(
                                !entry.chapters.is_empty(),
                                egui::Button::new(
                                    RichText::new(if entry.progress.is_some() {
                                        l.text("继续阅读", "Continue")
                                    } else {
                                        l.text("开始阅读", "Start reading")
                                    })
                                    .color(BACKGROUND)
                                    .strong(),
                                )
                                .fill(ACCENT),
                            )
                            .clicked()
                        {
                            self.resume(entry.clone(), settings);
                        }
                        if ui.button(l.text("刷新目录", "Refresh chapters")).clicked() {
                            self.open_book(shown.clone());
                        }
                    }
                });
                egui::CollapsingHeader::new(l.text("简介", "Description"))
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label(&shown.description);
                    });
            });
        });
        if self.detail_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(l.text("正在获取目录…", "Loading chapters…"));
            });
        }
        let Some(entry) = self.selected_entry() else {
            if self.detail_job.is_none() && ui.button(l.text("重试", "Retry")).clicked() {
                self.open_book(book);
            }
            return;
        };
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} {}",
                entry.chapters.len(),
                l.text("个章节", "chapters")
            ));
            ui.checkbox(&mut self.reverse, l.text("倒序", "Descending"));
            ui.checkbox(&mut self.unread_only, l.text("只看未读", "Unread only"));
            if ui.button(l.text("全部标为已读", "Mark all read")).clicked() {
                let mut entry = entry.clone();
                for chapter in entry.chapters.clone() {
                    entry.mark_read(chapter.key());
                }
                self.save(entry);
            }
        });
        let mut chapters: Vec<_> = entry
            .chapters
            .iter()
            .enumerate()
            .filter(|(_, c)| !self.unread_only || !entry.read.contains(c.key()))
            .map(|(i, c)| (i, c.clone()))
            .collect();
        if self.reverse {
            chapters.reverse();
        }
        let mut read = None;
        let mut mark = None;
        egui::ScrollArea::vertical()
            .id_salt(("comic_chapters", book.key()))
            .max_height((ui.available_height() - 65.0).max(80.0))
            .show(ui, |ui| {
                for special in [false, true] {
                    let list: Vec<_> = chapters
                        .iter()
                        .filter(|(_, c)| is_special(&c.name) == special)
                        .collect();
                    if list.is_empty() {
                        continue;
                    }
                    ui.label(
                        RichText::new(if special {
                            l.text("番外 / 附录", "Extras / appendices")
                        } else {
                            l.text("正文", "Chapters")
                        })
                        .strong()
                        .color(MUTED),
                    );
                    let columns = (ui.available_width() / 170.0).floor().max(1.0) as usize;
                    let width =
                        (ui.available_width() - 12.0 * (columns - 1) as f32) / columns as f32;
                    for row in list.chunks(columns) {
                        ui.horizontal(|ui| {
                            for (index, chapter) in row {
                                let prefix = if entry.new_unread.contains(chapter.key()) {
                                    "● "
                                } else if entry.read.contains(chapter.key()) {
                                    "✓ "
                                } else {
                                    ""
                                };
                                let current = entry
                                    .progress
                                    .as_ref()
                                    .is_some_and(|p| p.chapter == chapter.key());
                                let response = ui
                                    .add_sized(
                                        vec2(width, 46.0),
                                        egui::Button::new(
                                            RichText::new(format!("{prefix}{}", chapter.name))
                                                .color(if current {
                                                    ACCENT
                                                } else if entry.read.contains(chapter.key()) {
                                                    MUTED
                                                } else {
                                                    design::TEXT
                                                }),
                                        )
                                        .truncate()
                                        .fill(if current { design::SELECTED } else { SURFACE })
                                        .stroke(
                                            Stroke::new(
                                                1.0,
                                                if current { ACCENT } else { design::LINE },
                                            ),
                                        ),
                                    )
                                    .on_hover_text(&chapter.name);
                                if response.clicked() {
                                    read = Some(*index);
                                }
                                response.context_menu(|ui| {
                                    if ui.button(l.text("标记已读", "Mark read")).clicked() {
                                        mark = Some(chapter.key().to_owned());
                                        ui.close();
                                    }
                                });
                            }
                        });
                    }
                }
            });
        if let Some(index) = read {
            self.read_chapter(index, settings, false);
        }
        if let Some(chapter) = mark {
            let mut entry = entry;
            entry.mark_read(&chapter);
            self.save(entry);
        }
    }
}
fn is_special(name: &str) -> bool {
    [
        "番外", "附录", "附錄", "特典", "特别", "特別", "extra", "omake",
    ]
    .iter()
    .any(|s| name.to_lowercase().contains(s))
}
