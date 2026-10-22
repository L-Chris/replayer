use super::*;

impl State {
    pub(super) fn catalog(&mut self, ui: &mut Ui, settings: &crate::settings::Settings) {
        let l = settings.language;
        let updates = self
            .entries()
            .iter()
            .filter(|e| e.favorite && !e.new_unread.is_empty())
            .count();
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
        let compact = header.width() < 540.0;
        let action_width = if compact {
            34.0
        } else {
            design::control_width(ui, l.text("检查更新", "Check updates"), true)
        };
        let mut tabs = workspace::child(
            ui,
            "comic-catalog-tabs",
            Rect::from_min_max(header.min, header.max - vec2(action_width + 12.0, 0.0)),
        );
        let mut selected = match self.tab {
            Tab::Shelf => 0,
            Tab::Search => 1,
            Tab::Updates => 2,
            Tab::History => 3,
        };
        design::tabs(
            &mut tabs,
            &mut selected,
            &[
                l.text("书架", "Library"),
                l.text("搜索", "Search"),
                &format!(
                    "{}{}",
                    l.text("更新", "Updates"),
                    if updates > 0 {
                        format!(" · {updates}")
                    } else {
                        String::new()
                    }
                ),
                l.text("历史", "History"),
            ],
        );
        self.tab = [Tab::Shelf, Tab::Search, Tab::Updates, Tab::History][selected];
        let mut action = workspace::child(
            ui,
            "comic-check-updates",
            Rect::from_min_size(
                pos2(header.max.x - action_width, header.min.y),
                vec2(action_width, 34.0),
            ),
        );
        action.add_enabled_ui(
            self.update_job.is_none() && self.update_queue.is_empty(),
            |ui| {
                if design::control(
                    ui,
                    if compact {
                        ""
                    } else {
                        l.text("检查更新", "Check updates")
                    },
                    Some(|p, r, c| draw_repeat(p, r, c, false)),
                    design::Control::Outline,
                    action_width,
                )
                .on_hover_text(l.text("检查收藏更新", "Check favorite comics for updates"))
                .clicked()
                {
                    self.begin_updates(true);
                }
            },
        );
        ui.add_space(6.0);
        if self.update_job.is_some() || !self.update_queue.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.small(format!(
                    "{} {}/{}",
                    l.text("检查更新中", "Checking updates"),
                    self.update_done,
                    self.update_total
                ));
            });
        }
        if self.tab == Tab::Search {
            self.search_controls(ui, settings);
            let groups: Vec<_> = self
                .groups
                .iter()
                .filter(|g| self.site_filter.is_empty() || g.site.id == self.site_filter)
                .cloned()
                .collect();
            let books: Vec<_> = groups
                .iter()
                .flat_map(|g| g.books.iter().cloned())
                .collect();
            let failures: Vec<_> = groups
                .iter()
                .filter_map(|g| g.error.as_ref().map(|e| format!("{}: {e}", g.site.name)))
                .collect();
            let summary = |ui: &mut Ui| {
                if !self.search.is_empty() {
                    ui.spinner();
                    ui.label(RichText::new(l.text("正在搜索…", "Searching…")).color(MUTED));
                } else {
                    ui.label(
                        RichText::new(format!(
                            "{} {} {} · {} {}",
                            l.text("找到", "Found"),
                            books.len(),
                            l.text("部作品", "comics"),
                            groups.len(),
                            l.text("个来源", "sources")
                        ))
                        .small()
                        .color(MUTED),
                    );
                }
                if !failures.is_empty() {
                    ui.label(
                        RichText::new(l.text("部分来源暂不可用", "Some sources are unavailable"))
                            .small()
                            .color(Color32::from_rgb(218, 177, 126)),
                    )
                    .on_hover_text(failures.join("\n"));
                }
            };
            if header.width() >= 660.0 {
                let summary_ui = workspace::child(
                    ui,
                    "comic-search-summary",
                    Rect::from_min_max(
                        pos2(header.max.x - action_width - 282.0, header.min.y),
                        pos2(header.max.x - action_width - 16.0, header.max.y),
                    ),
                );
                let summary_rect = summary_ui.max_rect();
                let text = if !self.search.is_empty() {
                    l.text("正在搜索…", "Searching…").to_owned()
                } else {
                    format!(
                        "{} {} {} · {} {}",
                        l.text("找到", "Found"),
                        books.len(),
                        l.text("部作品", "comics"),
                        groups.len(),
                        l.text("个来源", "sources")
                    )
                };
                summary_ui.painter().text(
                    pos2(
                        summary_rect.min.x,
                        summary_rect.center().y - if failures.is_empty() { 0.0 } else { 7.0 },
                    ),
                    egui::Align2::LEFT_CENTER,
                    text,
                    egui::FontId::proportional(12.0),
                    MUTED,
                );
                if !failures.is_empty() {
                    summary_ui.painter().text(
                        pos2(summary_rect.min.x, summary_rect.center().y + 9.0),
                        egui::Align2::LEFT_CENTER,
                        l.text("部分来源暂不可用", "Some sources are unavailable"),
                        egui::FontId::proportional(11.0),
                        Color32::from_rgb(218, 177, 126),
                    );
                    summary_ui
                        .interact(summary_rect, summary_ui.id().with("errors"), Sense::hover())
                        .on_hover_text(failures.join("\n"));
                }
            } else {
                ui.horizontal_wrapped(summary);
            }
            ui.add_space(4.0);
            design::scroll()
                .id_salt("comic_search_results")
                .show(ui, |ui| {
                    if books.is_empty() && self.search.is_empty() {
                        self.catalog_empty(
                            ui,
                            l.text("寻找下一段故事", "Find your next story"),
                            l.text(
                                "输入漫画名称，搜索多个来源",
                                "Search across comic sources by title",
                            ),
                        );
                    }
                    self.book_grid(ui, &books, settings);
                });
        } else {
            design::scroll()
                .id_salt(("comic_library", self.tab as u8))
                .show(ui, |ui| {
                    if self.tab == Tab::Shelf
                        && self
                            .entries()
                            .iter()
                            .any(|e| e.progress.is_some() && e.last_read > 0)
                    {
                        self.continue_tile(ui, ui.available_width(), 180.0, settings);
                        ui.add_space(20.0);
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
                    let title = match self.tab {
                        Tab::Shelf => l.text("我的书架", "My library"),
                        Tab::History => l.text("最近阅读", "Recently read"),
                        _ => l.text("漫画更新", "Comic updates"),
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).size(20.0).strong());
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                entries.len(),
                                l.text("部作品", "comics")
                            ))
                            .small()
                            .color(MUTED),
                        );
                    });
                    ui.add_space(12.0);
                    if entries.is_empty() {
                        let title = if self.tab == Tab::Updates {
                            l.text("暂无新章节", "You're up to date")
                        } else if self.tab == Tab::History {
                            l.text("还没有阅读记录", "No reading history yet")
                        } else {
                            l.text("书架还是空的", "Your library is empty")
                        };
                        self.catalog_empty(
                            ui,
                            title,
                            l.text(
                                "搜索并收藏喜欢的漫画，在这里继续阅读",
                                "Search and favorite comics to continue reading here",
                            ),
                        );
                        if design::control(
                            ui,
                            l.text("搜索漫画", "Search comics"),
                            Some(draw_search),
                            design::Control::Primary,
                            0.0,
                        )
                        .clicked()
                        {
                            self.tab = Tab::Search;
                        }
                    }
                    if self.tab == Tab::Updates {
                        for entry in &entries {
                            if let Some(error) = &entry.check_error {
                                ui.label(
                                    RichText::new(format!("{} · {error}", entry.book.title))
                                        .color(Color32::LIGHT_RED),
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

    fn search_controls(&mut self, ui: &mut Ui, settings: &crate::settings::Settings) {
        let l = settings.language;
        let width = ui.available_width();
        let button_width = design::control_width(
            ui,
            if !self.search.is_empty() {
                l.text("取消", "Cancel")
            } else {
                l.text("搜索", "Search")
            },
            false,
        );
        let mut sites = self.sites.clone();
        for group in &self.groups {
            if !sites.iter().any(|site| site.id == group.site.id) {
                sites.push(group.site.clone());
            }
        }
        sites.sort_by_key(|site| site.id != "komiic:komiic");
        let selected = sites
            .iter()
            .find(|site| site.id == self.site_filter)
            .map(|s| s.name.as_str())
            .unwrap_or(l.text("全部来源", "All sources"));
        // Leave room for the two-pixel inset around the embedded selector.
        let source_width = (design::combo_width(ui, selected) + 8.0).clamp(88.0, 156.0);
        let mut submit = false;
        ui.horizontal(|ui| {
            let field_width = (width - button_width - 12.0).max(160.0);
            let (field, hover) =
                ui.allocate_exact_size(vec2(field_width, design::CONTROL_HEIGHT), Sense::hover());
            ui.painter().rect_filled(field, 8, design::FIELD);
            let menu_style = ui.style().clone();
            let mut source_ui = workspace::child(
                ui,
                "comic-source-prefix",
                Rect::from_min_size(
                    field.min + vec2(2.0, 2.0),
                    vec2(source_width - 4.0, design::CONTROL_HEIGHT - 4.0),
                ),
            );
            source_ui.spacing_mut().interact_size.y = design::CONTROL_HEIGHT - 4.0;
            source_ui.spacing_mut().button_padding.y = 6.0;
            let widgets = &mut source_ui.visuals_mut().widgets;
            for visuals in [
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
                &mut widgets.open,
            ] {
                visuals.bg_stroke = Stroke::NONE;
                visuals.weak_bg_fill = Color32::TRANSPARENT;
            }
            egui::ComboBox::from_id_salt("comic_source")
                .width(source_width - 4.0)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .selected_text(selected)
                .show_ui(&mut source_ui, |ui| {
                    ui.set_style(menu_style.clone());
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    design::selectable_value(
                        ui,
                        &mut self.site_filter,
                        String::new(),
                        l.text("全部来源", "All sources"),
                    );
                    for site in &sites {
                        design::selectable_value(
                            ui,
                            &mut self.site_filter,
                            site.id.clone(),
                            &site.name,
                        );
                    }
                });
            let divider = field.min.x + source_width;
            ui.painter().line_segment(
                [
                    pos2(divider, field.min.y + 8.0),
                    pos2(divider, field.max.y - 8.0),
                ],
                Stroke::new(1.0, design::BORDER),
            );
            let mut input_ui = workspace::child(
                ui,
                "comic-search-text",
                Rect::from_min_max(pos2(divider, field.min.y), field.max),
            );
            let input = design::search_input(
                &mut input_ui,
                &mut self.keyword,
                field_width - source_width,
                l.text("搜索漫画名称", "Search comics"),
            );
            let border = if input.has_focus() {
                ACCENT
            } else if hover.hovered() {
                MUTED
            } else {
                design::BORDER
            };
            ui.painter()
                .rect_stroke(field, 8, Stroke::new(1.0, border), egui::StrokeKind::Inside);
            submit = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if !self.search.is_empty() {
                if design::control(
                    ui,
                    l.text("取消", "Cancel"),
                    None,
                    design::Control::Outline,
                    button_width,
                )
                .clicked()
                {
                    self.search.clear();
                }
            } else {
                submit |= design::control(
                    ui,
                    l.text("搜索", "Search"),
                    None,
                    design::Control::Primary,
                    button_width,
                )
                .clicked();
            }
        });
        if submit && !self.keyword.trim().is_empty() {
            self.search_for(self.keyword.clone());
        }
        ui.add_space(4.0);
    }

    fn catalog_empty(&self, ui: &mut Ui, title: &str, subtitle: &str) {
        ui.add_space(32.0);
        let (icon, _) = ui.allocate_exact_size(vec2(30.0, 34.0), Sense::hover());
        reader::reader_book(ui.painter(), icon.shrink(3.0), ACCENT);
        ui.add_space(10.0);
        ui.label(RichText::new(title).size(21.0).strong());
        ui.label(RichText::new(subtitle).color(MUTED));
        ui.add_space(16.0);
    }

    pub(super) fn book_grid(
        &mut self,
        ui: &mut Ui,
        books: &[Book],
        settings: &crate::settings::Settings,
    ) -> bool {
        let l = settings.language;
        let columns = ((ui.available_width() + 16.0) / 164.0).floor().max(1.0) as usize;
        let width = ((ui.available_width() - 16.0 * columns.saturating_sub(1) as f32)
            / columns as f32)
            .max(60.0);
        let mut open = None;
        let mut favorite = None;
        for row in books.chunks(columns) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for book in row {
                    ui.push_id(book.key(), |ui| {
                        let cover_height = (width - 2.0 * design::CARD_PADDING) / COVER_RATIO;
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(width, cover_height + 72.0 + 2.0 * design::CARD_PADDING),
                            Sense::click(),
                        );
                        design::card_hover(ui, &response, 8);
                        let content = design::card_content(rect);
                        let cover =
                            Rect::from_min_size(content.min, vec2(content.width(), cover_height));
                        ui.painter().rect_filled(cover, 8, SURFACE);
                        if let Some(picture) = self.covers.get(&book.key()) {
                            picture.cover(ui, cover, 8);
                        } else {
                            reader::reader_book(
                                ui.painter(),
                                Rect::from_center_size(cover.center(), vec2(28.0, 34.0)),
                                MUTED,
                            );
                        }
                        if ui.clip_rect().intersects(cover) {
                            self.ensure_cover(book);
                        }
                        let entry = self.library.as_ref().and_then(|lib| lib.get(&book.key()));
                        let selected = entry.is_some_and(|e| e.favorite);
                        if entry.is_some_and(|e| e.favorite && !e.new_unread.is_empty()) {
                            ui.painter().circle_filled(
                                cover.min + vec2(12.0, 12.0),
                                4.0,
                                Color32::from_rgb(246, 91, 109),
                            );
                        }
                        let mut text = workspace::child(
                            ui,
                            "book-label",
                            Rect::from_min_max(pos2(content.min.x, cover.max.y + 9.0), content.max),
                        );
                        text.style_mut().interaction.selectable_labels = false;
                        text.spacing_mut().item_spacing.y = 3.0;
                        text.add(egui::Label::new(RichText::new(&book.title).strong()).truncate());
                        let subtitle = if let Some(entry) = entry.filter(|e| !e.chapters.is_empty())
                        {
                            format!(
                                "{} · {} {}",
                                book.site_name,
                                entry.chapters.len(),
                                l.text("话", "chapters")
                            )
                        } else {
                            book.site_name.clone()
                        };
                        text.add(
                            egui::Label::new(RichText::new(subtitle).small().color(MUTED))
                                .truncate(),
                        );
                        if let Some(entry) = entry {
                            let status = if !entry.new_unread.is_empty() {
                                format!(
                                    "{} {}",
                                    entry.new_unread.len(),
                                    l.text("新章节未读", "new unread chapters")
                                )
                            } else if let Some(p) = &entry.progress {
                                format!("{} {}", l.text("读到第", "Page"), p.page + 1)
                            } else {
                                String::new()
                            };
                            if !status.is_empty() {
                                text.add(
                                    egui::Label::new(
                                        RichText::new(status).size(11.0).color(ACCENT),
                                    )
                                    .truncate(),
                                );
                            }
                        }
                        let fav_rect = Rect::from_min_size(
                            pos2(cover.max.x - 40.0, cover.min.y + 8.0),
                            vec2(32.0, 32.0),
                        );
                        ui.painter()
                            .rect_filled(fav_rect, 7, Color32::from_black_alpha(170));
                        if design::favorite_at(ui, ui.id().with("favorite"), fav_rect, selected)
                            .on_hover_text(l.text("收藏 / 取消收藏", "Toggle favorite"))
                            .clicked()
                        {
                            favorite = Some(book.clone());
                        }
                        if response.on_hover_text(&book.title).clicked() {
                            open = Some(book.clone());
                        }
                    });
                }
            });
            ui.add_space(12.0);
        }
        if let Some(book) = favorite {
            let mut entry = self
                .library
                .as_ref()
                .and_then(|lib| lib.get(&book.key()))
                .cloned()
                .unwrap_or_else(|| Entry {
                    book,
                    ..Default::default()
                });
            entry.set_favorite(!entry.favorite);
            self.save(entry);
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
        if design::back_button(ui, l.text("返回书架 / 搜索", "Back to library / search")).clicked()
        {
            self.view = View::Catalog;
            self.detail_job = None;
            return;
        }
        let key = self.selected.as_ref().map(Book::key);
        design::scroll()
            .id_salt(("comic-detail", key))
            .show(ui, |ui| self.detail_contents(ui, settings));
    }

    fn detail_contents(&mut self, ui: &mut Ui, settings: &crate::settings::Settings) {
        let l = settings.language;
        let Some(book) = self.selected.clone() else {
            return;
        };
        let entry = self.selected_entry();
        let shown = entry.as_ref().map(|e| &e.book).unwrap_or(&book).clone();
        ui.horizontal_top(|ui| {
            let width = (ui.available_width() * 0.22).clamp(80.0, 190.0);
            let (rect, _) =
                ui.allocate_exact_size(vec2(width, width / COVER_RATIO), Sense::hover());
            ui.painter().rect_filled(rect, 8, SURFACE);
            if let Some(picture) = self.covers.get(&shown.key()) {
                picture.cover(ui, rect, 8);
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
                        if design::control(
                            ui,
                            if entry.favorite {
                                l.text("已收藏", "Favorited")
                            } else {
                                l.text("收藏", "Favorite")
                            },
                            Some(|p, r, c| design::heart(p, r, c, false)),
                            design::Control::Outline,
                            0.0,
                        )
                        .clicked()
                        {
                            let mut entry = entry.clone();
                            entry.set_favorite(!entry.favorite);
                            self.save(entry);
                        }
                        ui.add_enabled_ui(!entry.chapters.is_empty(), |ui| {
                            if design::control(
                                ui,
                                if entry.progress.is_some() {
                                    l.text("继续阅读", "Continue")
                                } else {
                                    l.text("开始阅读", "Start reading")
                                },
                                Some(reader::reader_book),
                                design::Control::Primary,
                                0.0,
                            )
                            .clicked()
                            {
                                self.resume(entry.clone(), settings);
                            }
                        });
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
            if entry
                .chapters
                .iter()
                .any(|c| c.kind == domain::ChapterKind::Volume)
            {
                let label = match self.chapter_filter {
                    Some(domain::ChapterKind::Chapter) => l.text("单话", "Chapters"),
                    Some(domain::ChapterKind::Volume) => l.text("单行本", "Volumes"),
                    None => l.text("全部", "All"),
                };
                egui::ComboBox::from_id_salt("comic-chapter-kind")
                    .width(design::combo_width(ui, label))
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (kind, name) in [
                            (
                                Some(domain::ChapterKind::Chapter),
                                l.text("单话", "Chapters"),
                            ),
                            (
                                Some(domain::ChapterKind::Volume),
                                l.text("单行本", "Volumes"),
                            ),
                            (None, l.text("全部", "All")),
                        ] {
                            design::selectable_value(ui, &mut self.chapter_filter, kind, name);
                        }
                    });
            }
            ui.checkbox(&mut self.reverse, l.text("倒序", "Descending"));
            ui.checkbox(&mut self.unread_only, l.text("只看未读", "Unread only"));
        });
        let mut chapters: Vec<_> = entry
            .chapters
            .iter()
            .enumerate()
            .filter(|(_, c)| self.chapter_filter.is_none_or(|kind| kind == c.kind))
            .filter(|(_, c)| !self.unread_only || !entry.read.contains(c.key()))
            .map(|(i, c)| (i, c.clone()))
            .collect();
        if self.reverse {
            chapters.reverse();
        }
        let mut read = None;
        let mut mark = None;
        ui.scope(|ui| {
            for section in 0..3 {
                let list: Vec<_> = chapters
                    .iter()
                    .filter(|(_, c)| {
                        let category = if c.kind == domain::ChapterKind::Volume {
                            2
                        } else {
                            usize::from(is_special(&c.name))
                        };
                        category == section
                    })
                    .collect();
                if list.is_empty() {
                    continue;
                }
                ui.label(
                    RichText::new(match section {
                        1 => l.text("番外 / 附录", "Extras / appendices"),
                        2 => l.text("单行本", "Volumes"),
                        _ => l.text("正文", "Chapters"),
                    })
                    .strong()
                    .color(MUTED),
                );
                let columns = (ui.available_width() / 170.0).floor().max(1.0) as usize;
                let width = (ui.available_width() - 12.0 * (columns - 1) as f32) / columns as f32;
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
                                        RichText::new(format!("{prefix}{}", chapter.name)).color(
                                            if current {
                                                ACCENT
                                            } else if entry.read.contains(chapter.key()) {
                                                MUTED
                                            } else {
                                                design::TEXT
                                            },
                                        ),
                                    )
                                    .truncate()
                                    .selected(current)
                                    .stroke(Stroke::new(
                                        1.0,
                                        if current { ACCENT } else { design::LINE },
                                    )),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
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
