use super::*;

impl App {
    pub(super) fn home_page(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        let query = self.workspace.query.trim().to_lowercase();
        page_heading(
            ui,
            l.text("为你续上精彩", "Pick up where you left off"),
            None,
        );
        design::scroll().auto_shrink([false, false]).show(ui, |ui| {
            let records: Vec<_> = self
                .records()
                .into_iter()
                .filter(|r| {
                    r.played > 0
                        && (query.is_empty()
                            || format!("{} {} {}", r.title, r.artist, r.album)
                                .to_lowercase()
                                .contains(&query))
                })
                .collect();
            let video = records.iter().find(|r| !r.music).cloned();
            let music = records.iter().find(|r| r.music).cloned();
            let width = ui.available_width();
            let height = (width * 0.29).clamp(180.0, 290.0);
            if width < 640.0 {
                self.continue_card(ui, video.as_ref().or(music.as_ref()), width, height);
            } else {
                ui.horizontal_top(|ui| {
                    let large = (width * 0.62 - 8.0).max(180.0);
                    self.continue_card(ui, video.as_ref().or(music.as_ref()), large, height);
                    ui.add_space(2.0);
                    let small_width = (width - large - 16.0).max(100.0);
                    if video.is_some() && music.is_some() {
                        self.continue_card(ui, music.as_ref(), small_width, height);
                    } else if self
                        .comic
                        .continue_tile(ui, small_width, height, &self.settings)
                    {
                        self.navigate(Page::Comic);
                    }
                });
            }
            ui.add_space(22.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(l.text("最近打开", "Recently opened"))
                        .size(20.0)
                        .strong(),
                );
                ui.add_space(6.0);
                let labels = [
                    l.text("全部", "All"),
                    l.text("音乐", "Music"),
                    l.text("视频", "Video"),
                    l.text("漫画", "Comics"),
                ];
                design::segmented(ui, &mut self.workspace.filter, &labels);
            });
            ui.add_space(12.0);
            if self.workspace.filter != 3 {
                let filtered: Vec<_> = records
                    .into_iter()
                    .filter(|r| {
                        self.workspace.filter == 0 || r.music == (self.workspace.filter == 1)
                    })
                    .take(8)
                    .collect();
                self.media_cards(ui, &filtered);
            }
            if self.workspace.filter == 0 || self.workspace.filter == 3 {
                ui.add_space(18.0);
                if self.comic.home_preview_filtered(ui, &self.settings, &query) {
                    self.navigate(Page::Comic);
                }
            }
        });
    }
    fn continue_card(&mut self, ui: &mut Ui, record: Option<&Record>, width: f32, height: f32) {
        let l = self.settings.language;
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        ui.painter().rect_filled(rect, 10, SURFACE);
        design::card_hover(ui, &response, 10);
        if let Some(record) = record {
            let content = rect;
            self.record_art(ui, content, record);
            let overlay =
                Rect::from_min_max(pos2(content.min.x, content.max.y - 104.0), content.max);
            ui.painter().rect_filled(
                overlay,
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 10,
                    se: 10,
                },
                Color32::from_black_alpha(205),
            );
            let mut text = child(
                ui,
                (&record.path, "continue"),
                overlay.shrink2(vec2(18.0, 12.0)),
            );
            text.style_mut().interaction.selectable_labels = false;
            text.label(
                RichText::new(if record.music {
                    l.text("继续聆听", "Continue listening")
                } else {
                    l.text("继续观看", "Continue watching")
                })
                .small()
                .color(ACCENT),
            );
            text.add(egui::Label::new(RichText::new(&record.title).size(20.0).strong()).truncate());
            text.label(
                RichText::new(if record.artist.is_empty() {
                    fmt_time(record.position)
                } else {
                    record.artist.clone()
                })
                .small()
                .color(MUTED),
            );
            if record.duration > 0.0 {
                ui.painter().rect_filled(
                    Rect::from_min_size(
                        pos2(content.min.x + 18.0, content.max.y - 8.0),
                        vec2(
                            (content.width() - 36.0)
                                * (record.position / record.duration).clamp(0.0, 1.0) as f32,
                            3.0,
                        ),
                    ),
                    2,
                    ACCENT,
                );
            }
            response.context_menu(|ui| {
                if design::menu_item(ui, false, l.text("删除播放记录", "Remove playback history"))
                    .clicked()
                {
                    self.remove_history(&record.path);
                    ui.close();
                }
            });
            if response
                .on_hover_text(l.text("继续播放", "Resume playback"))
                .clicked()
            {
                self.play_record(record.clone(), true);
            }
        } else {
            let mut text = child(
                ui,
                ("empty-continue", rect.min.x.to_bits()),
                rect.shrink(22.0),
            );
            let (icon, _) = text.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
            draw_folder(text.painter(), icon, ACCENT);
            text.add_space(12.0);
            text.label(
                RichText::new(l.text("打开或拖入本地媒体", "Open or drop local media"))
                    .size(18.0)
                    .strong(),
            );
            if response.clicked() && !self.dialog_open {
                self.open_dialog();
            }
        }
    }
}
