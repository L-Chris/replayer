use super::*;

impl App {
    pub(super) fn home_page(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        page_heading(
            ui,
            l.text("为你续上精彩", "Pick up where you left off"),
            Some(l.text(
                "从上次停下的地方，继续。",
                "Your music, films and stories, together.",
            )),
        );
        design::scroll().auto_shrink([false, false]).show(ui, |ui| {
            let records: Vec<_> = self
                .records()
                .into_iter()
                .filter(|r| r.played > 0)
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
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(l.text("最近打开", "Recently opened"))
                        .size(21.0)
                        .strong(),
                );
                for (i, zh, en) in [
                    (0, "全部", "All"),
                    (1, "音乐", "Music"),
                    (2, "视频", "Video"),
                    (3, "漫画", "Comics"),
                ] {
                    design::selectable_value(ui, &mut self.workspace.filter, i, l.text(zh, en));
                }
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
                if self.comic.home_preview(ui, &self.settings) {
                    self.navigate(Page::Comic);
                }
            }
        });
    }
    fn continue_card(&mut self, ui: &mut Ui, record: Option<&Record>, width: f32, height: f32) {
        let l = self.settings.language;
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        ui.painter().rect_filled(rect, 12, SURFACE);
        design::card_hover(ui, &response, 12);
        if let Some(record) = record {
            let content = design::card_content(rect);
            self.record_art(ui, content, record);
            let overlay =
                Rect::from_min_max(pos2(content.min.x, content.max.y - 104.0), content.max);
            ui.painter()
                .rect_filled(overlay, 0, Color32::from_black_alpha(205));
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
                if ui
                    .button(l.text("删除播放记录", "Remove playback history"))
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
            let (icon, _) = text.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
            draw_folder(text.painter(), icon, ACCENT);
            text.add_space(14.0);
            text.label(
                RichText::new(l.text("留一点时间给喜欢的内容", "Make room for what you love"))
                    .size(20.0)
                    .strong(),
            );
            text.label(
                RichText::new(l.text(
                    "打开本地媒体，或拖入文件开始播放",
                    "Open a local file or drop it here to begin",
                ))
                .color(MUTED),
            );
            if response.clicked() && !self.dialog_open {
                self.open_dialog();
            }
        }
    }
}
