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
        let action_width = design::CONTROL_HEIGHT;
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
                vec2(action_width, design::CONTROL_HEIGHT),
            ),
        );
        let updating = self.update_job.is_some() || !self.update_queue.is_empty();
        if updating {
            action.centered_and_justified(|ui| {
                ui.spinner();
            });
            action
                .interact(
                    action.max_rect(),
                    action.id().with("progress"),
                    Sense::hover(),
                )
                .on_hover_text(format!(
                    "{} {}/{}",
                    l.text("检查更新中", "Checking updates"),
                    self.update_done,
                    self.update_total
                ));
        } else if design::icon_button(
            &mut action,
            icons::ARROWS_CLOCKWISE,
            l.text("检查收藏更新", "Check favorite comics for updates"),
            false,
        )
        .clicked()
        {
            self.begin_updates(Instant::now());
        }
        ui.add_space(10.0);
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
            if !self.search.is_empty() || !books.is_empty() || !failures.is_empty() {
                ui.horizontal(|ui| {
                    if !self.search.is_empty() {
                        ui.spinner();
                        ui.small(RichText::new(l.text("正在搜索…", "Searching…")).color(MUTED));
                    } else if !books.is_empty() {
                        ui.small(
                            RichText::new(format!(
                                "{} {}",
                                books.len(),
                                l.text("部作品", "comics")
                            ))
                            .color(MUTED),
                        );
                    }
                    if !failures.is_empty() {
                        ui.small(
                            RichText::new(l.text("部分来源暂不可用", "Some sources unavailable"))
                                .color(Color32::from_rgb(218, 177, 126)),
                        )
                        .on_hover_text(failures.join("\n"));
                    }
                });
                ui.add_space(4.0);
            }
            design::scroll()
                .id_salt("comic_search_results")
                .show(ui, |ui| {
                    if books.is_empty() && self.search.is_empty() {
                        design::empty_state(
                            ui,
                            icons::MAGNIFYING_GLASS,
                            if self.keyword.trim().is_empty() {
                                l.text("输入漫画名称开始搜索", "Search comics by title")
                            } else {
                                l.text("未找到相关漫画", "No matching comics")
                            },
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
                    if self.tab == Tab::History {
                        entries.sort_by_key(|e| std::cmp::Reverse(e.last_read));
                    } else {
                        domain::library::sort_favorites(&mut entries);
                    }
                    let title = match self.tab {
                        Tab::Shelf => l.text("我的书架", "My library"),
                        Tab::History => l.text("最近阅读", "Recently read"),
                        _ => l.text("漫画更新", "Comic updates"),
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).size(20.0).strong());
                        if !entries.is_empty() {
                            ui.label(
                                RichText::new(entries.len().to_string())
                                    .small()
                                    .color(MUTED),
                            );
                        }
                    });
                    ui.add_space(12.0);
                    if entries.is_empty() {
                        let empty_title = if self.tab == Tab::Updates {
                            l.text("暂无新章节", "No new chapters")
                        } else if self.tab == Tab::History {
                            l.text("暂无阅读记录", "No reading history")
                        } else {
                            l.text("暂无收藏漫画", "No favorite comics")
                        };
                        design::empty_state(ui, icons::BOOK_OPEN, empty_title);
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
            .map(|s| s.name.clone())
            .unwrap_or_else(|| l.text("全部来源", "All sources").to_owned());
        let source_width = (design::combo_width(ui, &selected) + 12.0).clamp(104.0, 168.0);
        let mut submit = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            design::dropdown_ui(ui, "comic_source", source_width, &selected, |ui| {
                design::menu_value(
                    ui,
                    &mut self.site_filter,
                    String::new(),
                    l.text("全部来源", "All sources"),
                );
                for site in &sites {
                    design::menu_value(ui, &mut self.site_filter, site.id.clone(), &site.name);
                }
            });
            let field_width = (width - source_width - design::CONTROL_HEIGHT - 16.0).max(120.0);
            let input = design::search_field(
                ui,
                &mut self.keyword,
                field_width,
                l.text("搜索漫画名称", "Search comics"),
            );
            submit = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if !self.search.is_empty() {
                if design::icon_button(ui, icons::X, l.text("取消搜索", "Cancel search"), false)
                    .clicked()
                {
                    self.search.clear();
                }
            } else {
                submit |= design::primary_icon_button(
                    ui,
                    icons::MAGNIFYING_GLASS,
                    l.text("搜索", "Search"),
                )
                .clicked();
            }
        });
        if submit && !self.keyword.trim().is_empty() {
            self.search_for(self.keyword.clone());
        }
        ui.add_space(6.0);
    }

    pub(super) fn book_grid(
        &mut self,
        ui: &mut Ui,
        books: &[Book],
        settings: &crate::settings::Settings,
    ) -> bool {
        self.book_grid_with_controls(ui, books, settings, true)
    }

    pub(super) fn book_grid_with_controls(
        &mut self,
        ui: &mut Ui,
        books: &[Book],
        settings: &crate::settings::Settings,
        show_favorite: bool,
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
                            self.ensure_cover(book, ui.ctx());
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
                        if show_favorite {
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
            self.ensure_cover(&shown, ui.ctx());
            ui.vertical(|ui| {
                ui.label(RichText::new(&shown.title).size(29.0).strong());
                ui.label(format!("{} · {}", shown.author, shown.site_name));
                if !shown.status.is_empty() {
                    ui.small(&shown.status);
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if let Some(entry) = &entry {
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
                        let fav_tip = if entry.favorite {
                            l.text("取消收藏", "Remove favorite")
                        } else {
                            l.text("收藏", "Favorite")
                        };
                        let fav_filled = entry.favorite;
                        if design::icon_button_sized(
                            ui,
                            design::CONTROL_HEIGHT,
                            1.0,
                            fav_filled,
                            fav_tip,
                            |p, r, c| design::heart(p, r, c, fav_filled),
                        )
                        .clicked()
                        {
                            let mut entry = entry.clone();
                            entry.set_favorite(!entry.favorite);
                            self.save(entry);
                        }
                    }
                });
                if !shown.description.trim().is_empty() {
                    egui::CollapsingHeader::new(l.text("简介", "Description"))
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.label(&shown.description);
                        });
                }
            });
        });
        if self.detail_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(l.text("正在获取目录…", "Loading chapters…"));
            });
        }
        let Some(entry) = self.selected_entry() else {
            if self.detail_job.is_none()
                && design::control(
                    ui,
                    l.text("重试", "Retry"),
                    Some(icons::refresh),
                    design::Control::Outline,
                    0.0,
                )
                .clicked()
            {
                self.open_book(book);
            }
            return;
        };
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    entry.chapters.len(),
                    l.text("话", "chapters")
                ))
                .color(MUTED),
            );
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
                let drop_w = (design::combo_width(ui, label) + 12.0).max(96.0);
                design::dropdown_ui(ui, "comic-chapter-kind", drop_w, label, |ui| {
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
                        design::menu_value(ui, &mut self.chapter_filter, kind, name);
                    }
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if design::icon_button(
                    ui,
                    icons::FUNNEL,
                    l.text("只看未读", "Unread only"),
                    self.unread_only,
                )
                .clicked()
                {
                    self.unread_only = !self.unread_only;
                }
                if design::icon_button(
                    ui,
                    if self.reverse {
                        icons::SORT_DESCENDING
                    } else {
                        icons::SORT_ASCENDING
                    },
                    if self.reverse {
                        l.text(
                            "当前倒序（点击切为正序）",
                            "Descending (click for ascending)",
                        )
                    } else {
                        l.text(
                            "当前正序（点击切为倒序）",
                            "Ascending (click for descending)",
                        )
                    },
                    self.reverse,
                )
                .clicked()
                {
                    self.reverse = !self.reverse;
                }
            });
        });
        ui.add_space(6.0);
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
                ui.add_space(4.0);
                ui.label(
                    RichText::new(match section {
                        1 => l.text("番外 / 附录", "Extras"),
                        2 => l.text("单行本", "Volumes"),
                        _ => l.text("正文", "Chapters"),
                    })
                    .strong()
                    .color(MUTED),
                );
                let columns = (ui.available_width() / 170.0).floor().max(1.0) as usize;
                let width = (ui.available_width() - 10.0 * (columns - 1) as f32) / columns as f32;
                for row in list.chunks(columns) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        for (index, chapter) in row {
                            let is_current = entry
                                .progress
                                .as_ref()
                                .is_some_and(|p| p.chapter == chapter.key());
                            let is_new = entry.new_unread.contains(chapter.key());
                            let is_read = entry.read.contains(chapter.key());
                            let response =
                                chapter_card(ui, width, &chapter.name, is_current, is_new, is_read);
                            if response.clicked() {
                                read = Some(*index);
                            }
                            response.context_menu(|ui| {
                                if design::menu_item(ui, false, l.text("标记已读", "Mark read"))
                                    .clicked()
                                {
                                    mark = Some(chapter.key().to_owned());
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
fn chapter_card(
    ui: &mut Ui,
    width: f32,
    name: &str,
    current: bool,
    new_unread: bool,
    read: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 42.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), current, name)
    });
    let hovered = (response.hovered() || response.has_focus()) && ui.is_enabled();
    let fill = if current {
        design::SELECTED
    } else if hovered {
        design::HOVER
    } else {
        SURFACE
    };
    let stroke_color = if current {
        ACCENT
    } else if hovered {
        design::BORDER
    } else {
        design::LINE
    };
    ui.painter().rect_filled(rect, 8, fill);
    ui.painter().rect_stroke(
        rect,
        8,
        Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );
    let has_indicator = current || new_unread || read;
    if current {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.min.x + 17.0, rect.center().y), vec2(14.0, 14.0)),
            ACCENT,
            icons::BOOK_OPEN,
        );
    } else if new_unread {
        ui.painter()
            .circle_filled(pos2(rect.min.x + 17.0, rect.center().y), 3.5, ACCENT);
    } else if read {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.min.x + 17.0, rect.center().y), vec2(14.0, 14.0)),
            MUTED,
            icons::CHECK,
        );
    }
    let text_x = if has_indicator {
        rect.min.x + 30.0
    } else {
        rect.min.x + 12.0
    };
    let text_color = if current {
        ACCENT
    } else if read {
        MUTED
    } else {
        design::TEXT
    };
    let galley =
        ui.painter()
            .layout_no_wrap(name.into(), egui::FontId::proportional(13.5), text_color);
    let truncated = galley.size().x > (rect.max.x - 10.0 - text_x).max(0.0);
    let clip = Rect::from_min_max(
        pos2(text_x, rect.min.y),
        pos2(rect.max.x - 10.0, rect.max.y),
    )
    .intersect(ui.clip_rect());
    ui.painter().with_clip_rect(clip).galley(
        pos2(text_x, rect.center().y - galley.size().y * 0.5),
        galley,
        text_color,
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if truncated {
        response.on_hover_text(name)
    } else {
        response
    }
}

fn is_special(name: &str) -> bool {
    [
        "番外", "附录", "附錄", "特典", "特别", "特別", "extra", "omake",
    ]
    .iter()
    .any(|s| name.to_lowercase().contains(s))
}
