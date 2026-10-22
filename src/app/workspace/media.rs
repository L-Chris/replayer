use super::*;

impl App {
    pub(super) fn records(&self) -> Vec<Record> {
        let mut records = self.library.records.clone();
        for (queue, music) in [
            (&self.queue, self.queue_kind_music),
            (&self.queue_inactive, !self.queue_kind_music),
        ] {
            for item in &queue.items {
                if let crate::queue::Source::File(path) = &item.source
                    && !records.iter().any(|r| r.path == *path)
                {
                    records.push(Record {
                        path: path.clone(),
                        title: std::path::Path::new(&item.title)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| item.title.clone()),
                        music,
                        ..Default::default()
                    });
                }
            }
        }
        for record in &mut records {
            if let Some(Some(info)) = self.metadata_cache.get(&record.path) {
                if let Some(title) = &info.title {
                    record.title = title.clone();
                }
                if let Some(artist) = &info.artist {
                    record.artist = artist.clone();
                }
                if let Some(album) = &info.album {
                    record.album = album.clone();
                }
            }
        }
        records.sort_by_key(|r| std::cmp::Reverse(r.played));
        records
    }
    pub(super) fn play_record(&mut self, record: Record, resume: bool) {
        self.update_library();
        self.comic.flush();
        self.set_active_queue(record.music);
        let index = self
            .queue
            .items
            .iter()
            .position(
                |item| matches!(&item.source,crate::queue::Source::File(p) if *p==record.path),
            )
            .unwrap_or_else(|| {
                self.queue
                    .items
                    .push(crate::queue::Item::file(record.path.clone()));
                self.queue.items.len() - 1
            });
        self.workspace.resume = resume
            .then(|| record.resume_at())
            .flatten()
            .map(|position| (record.path.clone(), position));
        self.play_queue(index);
    }
    pub(super) fn record_art(&mut self, ui: &Ui, rect: Rect, record: &Record) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        self.metadata_cache
            .request(&record.path, &self.metadata_requests);
        let art = self
            .metadata_cache
            .get(&record.path)
            .and_then(|v| v.as_ref())
            .and_then(|i| i.artwork.clone());
        let texture = art.map(|art| {
            self.art_textures
                .entry(record.path.clone())
                .or_insert_with(|| {
                    ui.ctx().load_texture(
                        format!("library-{}", record.path),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [art.width, art.height],
                            &art.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                })
                .clone()
        });
        ui.painter().rect_filled(rect, 10, SURFACE);
        if let Some(texture) = texture {
            design::cover(ui, &texture, rect, 10);
        } else {
            let color = if record.music {
                Color32::from_rgb(45, 67, 62)
            } else {
                Color32::from_rgb(41, 53, 61)
            };
            ui.painter().rect_filled(rect, 10, color);
            let icon = Rect::from_center_size(
                rect.center(),
                Vec2::splat((rect.width() * 0.24).clamp(18.0, 56.0)),
            );
            if record.music {
                draw_music(ui.painter(), icon, ACCENT);
            } else {
                draw_video(ui.painter(), icon, MUTED);
            }
        }
    }

    pub(super) fn media_cards(&mut self, ui: &mut Ui, records: &[Record]) {
        let columns = (ui.available_width() / 180.0).floor().clamp(1.0, 5.0) as usize;
        let width =
            (ui.available_width() - 12.0 * columns.saturating_sub(1) as f32) / columns as f32;
        let mut play = None;
        let mut favorite = None;
        for row in records.chunks(columns) {
            ui.horizontal_top(|ui| {
                for record in row {
                    ui.push_id(&record.path, |ui| {
                        let art_height = width - 2.0 * design::CARD_PADDING;
                        let (card, response) = ui.allocate_exact_size(
                            vec2(width, art_height + 62.0 + 2.0 * design::CARD_PADDING),
                            Sense::click(),
                        );
                        design::card_hover(ui, &response, 10);
                        let content = design::card_content(card);
                        let art =
                            Rect::from_min_size(content.min, vec2(content.width(), art_height));
                        self.record_art(ui, art, record);
                        let mut text = child(
                            ui,
                            "media-card-label",
                            Rect::from_min_max(pos2(content.min.x, art.max.y + 8.0), content.max),
                        );
                        text.style_mut().interaction.selectable_labels = false;
                        text.spacing_mut().item_spacing.y = 3.0;
                        text.add(
                            egui::Label::new(RichText::new(&record.title).strong()).truncate(),
                        );
                        let subtitle = if record.artist.is_empty() {
                            self.settings
                                .language
                                .text(
                                    if record.music { "音乐" } else { "视频" },
                                    if record.music { "Music" } else { "Video" },
                                )
                                .to_owned()
                        } else {
                            record.artist.clone()
                        };
                        text.add(
                            egui::Label::new(RichText::new(subtitle).small().color(MUTED))
                                .truncate(),
                        );
                        let fav = Rect::from_min_size(
                            pos2(art.max.x - 40.0, art.min.y + 8.0),
                            Vec2::splat(32.0),
                        );
                        ui.painter()
                            .rect_filled(fav, 7, Color32::from_black_alpha(170));
                        if design::favorite_at(ui, ui.id().with("favorite"), fav, record.favorite)
                            .on_hover_text(
                                self.settings
                                    .language
                                    .text("收藏 / 取消收藏", "Toggle favorite"),
                            )
                            .clicked()
                        {
                            favorite = Some(record.clone());
                        }
                        response.context_menu(|ui| {
                            if ui
                                .button(
                                    self.settings
                                        .language
                                        .text("收藏 / 取消收藏", "Toggle favorite"),
                                )
                                .clicked()
                            {
                                favorite = Some(record.clone());
                                ui.close();
                            }
                        });
                        if response.clicked() {
                            play = Some(record.clone());
                        }
                    });
                }
            });
        }
        if let Some(record) = favorite {
            self.library
                .toggle_favorite(&record.path, &record.title, record.music);
        }
        if let Some(record) = play {
            self.play_record(record, true);
        }
    }

    pub(super) fn media_library(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        let page = self.workspace.page;
        let title = match page {
            Page::Music => l.text("音乐", "Music"),
            Page::Video => l.text("视频", "Video"),
            Page::Favorites => l.text("收藏", "Favorites"),
            Page::Downloads => l.text("下载", "Downloads"),
            _ => l.text("最近播放", "Recently played"),
        };
        page_heading(ui, title, None);
        if page == Page::Music {
            ui.horizontal(|ui| {
                for (i, zh, en) in [
                    (0, "本地音乐", "Local music"),
                    (1, "在线搜索", "Online search"),
                ] {
                    design::selectable_value(ui, &mut self.workspace.music_tab, i, l.text(zh, en));
                }
            });
            ui.add_space(12.0);
            if self.workspace.music_tab == 1 {
                self.online_music_contents(ui);
                return;
            }
        }
        if page == Page::Video {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        l.text("本地视频与播放历史", "Local videos and playback history"),
                    )
                    .color(MUTED),
                );
                if ui.button(l.text("打开磁链", "Open magnet")).clicked() {
                    self.magnet_open = true;
                }
            });
            ui.add_space(12.0);
        }
        let mut records = self.records();
        if page == Page::Downloads {
            for path in self.workspace.downloads.clone() {
                if !records.iter().any(|r| r.path == path) {
                    records.push(Record {
                        title: std::path::Path::new(&path)
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        path,
                        music: true,
                        ..Default::default()
                    });
                }
            }
        }
        let query = self.workspace.query.to_lowercase();
        records.retain(|r|match page{Page::Music=>r.music,Page::Video=>!r.music,Page::Favorites=>r.favorite,Page::Recent=>r.played>0,Page::Downloads=>std::path::Path::new(&r.path).starts_with(self.settings.music.directory()),_=>true}
            && (query.is_empty() || format!("{} {} {}",r.title,r.artist,r.album).to_lowercase().contains(&query)));
        ui.label(
            RichText::new(format!(
                "{} {}",
                records.len(),
                l.text("个媒体文件", "media files")
            ))
            .small()
            .color(MUTED),
        );
        ui.add_space(12.0);
        design::scroll().auto_shrink([false, false]).show(ui, |ui| {
            if page == Page::Video {
                self.media_cards(ui, &records);
            } else {
                if page == Page::Music && !records.is_empty() {
                    ui.label(
                        RichText::new(l.text("最近聆听", "Recently listened"))
                            .size(20.0)
                            .strong(),
                    );
                    ui.add_space(10.0);
                    self.media_cards(ui, &records.iter().take(4).cloned().collect::<Vec<_>>());
                    ui.add_space(16.0);
                }
                if !records.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(l.text("全部歌曲", "All tracks"))
                                .size(20.0)
                                .strong(),
                        );
                        if primary(ui, l.text("播放全部", "Play all")).clicked() {
                            self.play_record(records[0].clone(), false);
                        }
                    });
                    ui.add_space(12.0);
                }
                let (header, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                for (x, text) in [
                    (13.0, "#"),
                    (94.0, l.text("标题 / 艺术家", "Title / artist")),
                ] {
                    ui.painter().text(
                        pos2(header.min.x + x, header.center().y),
                        egui::Align2::LEFT_CENTER,
                        text,
                        egui::FontId::proportional(12.0),
                        MUTED,
                    );
                }
                if header.width() > 530.0 {
                    ui.painter().text(
                        pos2(
                            header.min.x + header.width() * 0.5 + 12.0,
                            header.center().y,
                        ),
                        egui::Align2::LEFT_CENTER,
                        l.text("专辑", "Album"),
                        egui::FontId::proportional(12.0),
                        MUTED,
                    );
                }
                for (index, record) in records.iter().enumerate() {
                    self.track_row(ui, record, index);
                }
            }
            if records.is_empty() {
                ui.add_space(44.0);
                ui.label(
                    RichText::new(l.text("这里还没有内容", "Nothing here yet"))
                        .size(22.0)
                        .strong(),
                );
                ui.label(
                    RichText::new(l.text(
                        "打开文件后自动加入资料库，也可以直接拖入窗口",
                        "Open files to add them to your library, or drag them into this window",
                    ))
                    .color(MUTED),
                );
            }
            if page == Page::Favorites {
                ui.add_space(20.0);
                if self.comic.favorite_preview(ui, &self.settings) {
                    self.navigate(Page::Comic);
                }
            }
        });
    }
    fn track_row(&mut self, ui: &mut Ui, record: &Record, index: usize) {
        let current = self
            .source
            .as_ref()
            .is_some_and(|s| s.to_string_lossy() == record.path);
        let (r, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if current || response.hovered() {
            ui.painter()
                .rect_filled(r, 7, if current { SELECTED } else { SURFACE });
        }
        ui.painter().text(
            pos2(r.min.x + 13.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            (index + 1).to_string(),
            egui::FontId::proportional(12.0),
            MUTED,
        );
        let art = Rect::from_min_size(r.min + vec2(38.0, 10.0), vec2(42.0, 42.0));
        self.record_art(ui, art, record);
        let end = (r.width() * 0.50).max(160.0);
        let mut text = child(
            ui,
            (&record.path, "track-title"),
            Rect::from_min_max(r.min + vec2(94.0, 10.0), pos2(r.min.x + end, r.max.y - 7.0)),
        );
        text.style_mut().interaction.selectable_labels = false;
        text.add(
            egui::Label::new(RichText::new(&record.title).color(if current {
                ACCENT
            } else {
                TEXT
            }))
            .truncate(),
        );
        text.add(egui::Label::new(RichText::new(&record.artist).small().color(MUTED)).truncate());
        if r.width() > 530.0 {
            let mut album = child(
                ui,
                (&record.path, "album"),
                Rect::from_min_max(
                    pos2(r.min.x + end + 12.0, r.min.y + 18.0),
                    pos2(r.max.x - 116.0, r.max.y),
                ),
            );
            album.add(egui::Label::new(RichText::new(&record.album).color(MUTED)).truncate());
        }
        let favorite_rect =
            Rect::from_center_size(pos2(r.max.x - 24.0, r.center().y), Vec2::splat(32.0));
        if record.duration > 0.0 {
            ui.painter().text(
                pos2(favorite_rect.min.x - 12.0, r.center().y),
                egui::Align2::RIGHT_CENTER,
                fmt_time(record.duration),
                egui::FontId::proportional(12.0),
                MUTED,
            );
        }
        let resp = design::favorite_at(
            ui,
            ui.id().with((&record.path, "favorite")),
            favorite_rect,
            record.favorite,
        );
        if resp
            .on_hover_text(self.settings.language.text("收藏", "Favorite"))
            .clicked()
        {
            self.library
                .toggle_favorite(&record.path, &record.title, record.music);
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &record.title)
        });
        if response.double_clicked() {
            self.play_record(record.clone(), false);
        }
        response.context_menu(|ui| {
            if ui
                .button(self.settings.language.text("播放", "Play"))
                .clicked()
            {
                self.play_record(record.clone(), false);
                ui.close();
            }
        });
    }
}
