use super::*;
use crate::queue::{Item, Source};

impl App {
    pub(super) fn remove_torrent_queue(&mut self) {
        let current = self.queue.current.and_then(|i| {
            self.queue.items.get(i).and_then(|item| {
                matches!(item.source, Source::File(_)).then(|| {
                    self.queue.items[..i]
                        .iter()
                        .filter(|item| matches!(item.source, Source::File(_)))
                        .count()
                })
            })
        });
        self.queue
            .items
            .retain(|item| matches!(item.source, Source::File(_)));
        self.queue.current = current;
    }
    pub(super) fn is_music(&self) -> bool {
        self.media_info.as_ref().map_or_else(
            || {
                self.source
                    .as_deref()
                    .is_some_and(crate::media_source::is_qq)
            },
            |info| info.kind == crate::media::Kind::Music,
        )
    }
    pub(super) fn open_items(&mut self, paths: Vec<String>, append: bool) {
        let items = paths
            .into_iter()
            .filter(|path| !std::path::Path::new(path).is_dir())
            .map(Item::file)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return;
        }
        if append {
            let start = self.queue.items.len();
            self.queue.items.extend(items);
            if self.player.is_none() {
                self.play_queue(start);
            }
        } else {
            self.queue.items = items;
            self.queue.current = None;
            self.play_queue(0);
        }
    }
    pub(super) fn play_queue(&mut self, index: usize) {
        let Some(item) = self.queue.select(index) else {
            return;
        };
        match item.source {
            Source::File(path) => self.load(path),
            Source::Torrent(id) => {
                if let Some(job) = &self.torrent_job {
                    self.player = None;
                    self.media_info = None;
                    self.artwork = None;
                    self.video.clear();
                    self.subtitle_job = None;
                    self.subtitles.clear();
                    self.source = None;
                    job.select(id);
                }
            }
        }
    }
    fn queue_contents(&self, ui: &mut Ui, act: &mut Act) {
        let language = self.settings.language;
        ui.horizontal(|ui| {
            if ui.button(language.text("添加文件", "Add files")).clicked() {
                *act = Act::AddFiles;
            }
        });
        ui.add_space(8.0);
        if self.queue.items.is_empty() {
            ui.label(language.text("队列为空", "Queue is empty"));
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, item) in self.queue.items.iter().enumerate() {
                    let current = self.queue.current == Some(index);
                    ui.push_id(index, |ui| {
                        egui::Frame::new()
                            .fill(if current {
                                Color32::from_rgb(34, 50, 77)
                            } else {
                                SURFACE
                            })
                            .corner_radius(8)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                                    ui.spacing_mut().interact_size = vec2(24.0, 28.0);
                                    if ui
                                        .add(egui::Button::new("▶"))
                                        .on_hover_text(language.text("播放此项", "Play item"))
                                        .clicked()
                                    {
                                        *act = Act::QueuePlay(index);
                                    }
                                    let label_width = (ui.available_width() - 100.0).max(40.0);
                                    ui.allocate_ui_with_layout(
                                        vec2(label_width, 32.0),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            ui.set_min_width(label_width);
                                            ui.add(
                                                egui::Label::new(RichText::new(&item.title).color(
                                                    if current { Color32::WHITE } else { MUTED },
                                                ))
                                                .truncate(),
                                            )
                                        },
                                    )
                                    .inner
                                    .on_hover_text(&item.title);
                                    if ui
                                        .add_enabled(index > 0, egui::Button::new("↑").small())
                                        .on_hover_text(language.text("上移", "Move up"))
                                        .clicked()
                                    {
                                        *act = Act::QueueMove(index, true);
                                    }
                                    if ui
                                        .add_enabled(
                                            index + 1 < self.queue.items.len(),
                                            egui::Button::new("↓").small(),
                                        )
                                        .on_hover_text(language.text("下移", "Move down"))
                                        .clicked()
                                    {
                                        *act = Act::QueueMove(index, false);
                                    }
                                    if ui
                                        .add_enabled(!current, egui::Button::new("×").small())
                                        .on_hover_text(language.text(
                                            "从队列移除（不删除文件）",
                                            "Remove from queue (keeps file)",
                                        ))
                                        .clicked()
                                    {
                                        *act = Act::QueueRemove(index);
                                    }
                                });
                            });
                        ui.add_space(4.0);
                    });
                }
            });
    }
    pub(super) fn queue_sidebar(&mut self, root: &mut Ui, rect: Rect, act: &mut Act) {
        root.painter().rect_filled(rect, 0, SURFACE);
        root.painter().line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, Color32::from_rgb(48, 57, 73)),
        );
        let mut ui = root.new_child(
            UiBuilder::new()
                .id_salt("queue-sidebar")
                .max_rect(rect.shrink(12.0))
                .layout(Layout::top_down(Align::Min)),
        );
        ui.set_clip_rect(rect.shrink(1.0));
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .button(self.settings.language.text("收起", "Hide"))
                    .clicked()
                {
                    self.queue_open = false;
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.label(
                        RichText::new(self.settings.language.text("播放队列", "Play queue"))
                            .strong(),
                    );
                    ui.label(RichText::new(self.queue.items.len().to_string()).color(MUTED));
                });
            });
        });
        ui.separator();
        self.queue_contents(&mut ui, act);
    }
    pub(super) fn music_view(
        &mut self,
        root: &mut Ui,
        screen: Rect,
        playback: Rect,
        act: &mut Act,
    ) {
        let language = self.settings.language;
        let content = Rect::from_min_max(
            playback.min + vec2(24.0, BAR_TOP + 20.0),
            playback.max - vec2(24.0, 126.0),
        );
        let left = content;
        let compact = left.height() < 280.0;
        let size = if compact {
            left.height().clamp(40.0, 110.0)
        } else {
            (left.height() - 112.0)
                .clamp(90.0, 280.0)
                .min(left.width() - 20.0)
        };
        let cover = if compact {
            Rect::from_min_size(left.min, Vec2::splat(size))
        } else {
            Rect::from_center_size(
                pos2(left.center().x, left.min.y + size / 2.0),
                Vec2::splat(size),
            )
        };
        let painter = root.painter();
        painter.rect_filled(cover, 16, SURFACE);
        if let Some(art) = &self.artwork {
            let texture_size = art.size_vec2();
            let ratio = (cover.width() / texture_size.x).min(cover.height() / texture_size.y);
            painter.image(
                art.id(),
                Rect::from_center_size(cover.center(), texture_size * ratio),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                cover.center(),
                egui::Align2::CENTER_CENTER,
                "♫",
                egui::FontId::proportional(size * 0.4),
                ACCENT,
            );
        }
        let text_rect = if compact {
            Rect::from_min_max(pos2(cover.max.x + 18.0, left.min.y), left.max)
        } else {
            Rect::from_min_max(pos2(left.min.x, cover.max.y + 16.0), left.max)
        };
        let mut info_ui = root.new_child(
            UiBuilder::new()
                .id_salt("music-info")
                .max_rect(text_rect)
                .layout(Layout::top_down(if compact {
                    Align::Min
                } else {
                    Align::Center
                })),
        );
        info_ui
            .add(
                egui::Label::new(
                    RichText::new(&self.title)
                        .size(if compact { 18.0 } else { 24.0 })
                        .strong(),
                )
                .truncate(),
            )
            .on_hover_text(&self.title);
        if let Some(info) = &self.media_info {
            if let Some(artist) = &info.artist {
                info_ui.label(RichText::new(artist).color(MUTED));
            }
            if let Some(album) = &info.album {
                info_ui.small(RichText::new(album).color(MUTED));
            }
        }
        let rect = Rect::from_min_max(pos2(screen.min.x, screen.max.y - 108.0), screen.max);
        root.painter().rect_filled(rect, 0, SURFACE);
        let mut ui = root.new_child(
            UiBuilder::new()
                .id_salt("music-controls")
                .max_rect(rect.shrink2(vec2(24.0, 10.0)))
                .layout(Layout::top_down(Align::Min)),
        );
        let (playing, duration, position) = self
            .player
            .as_ref()
            .map(|p| (p.is_playing(), p.duration(), p.position()))
            .unwrap_or((false, 0.0, 0.0));
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(fmt_time(if self.scrubbing { self.scrub } else { position }))
                    .monospace(),
            );
            let shown = if self.scrubbing { self.scrub } else { position };
            let (track, response) = hslider(
                ui,
                (ui.available_width() - 66.0).max(80.0),
                18.0,
                (shown / duration.max(0.001)).clamp(0.0, 1.0) as f32,
                3.0,
                true,
                1.0,
            );
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                self.scrubbing = true;
                self.scrub =
                    ((pointer.x - track.min.x) / track.width()).clamp(0.0, 1.0) as f64 * duration;
            }
            if response.drag_stopped() {
                *act = Act::Seek(self.scrub);
                self.scrubbing = false;
            } else if response.clicked()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                *act = Act::Seek(
                    ((pointer.x - track.min.x) / track.width()).clamp(0.0, 1.0) as f64 * duration,
                );
            }
            ui.label(RichText::new(fmt_time(duration)).monospace());
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.queue.previous_index().is_some() || position > 3.0,
                    egui::Button::new(language.text("上一首", "Previous")),
                )
                .clicked()
            {
                *act = Act::Previous;
            }
            if ui
                .add(
                    egui::Button::new(if playing {
                        language.text("暂停", "Pause")
                    } else {
                        language.text("播放", "Play")
                    })
                    .fill(ACCENT)
                    .min_size(vec2(76.0, 34.0)),
                )
                .clicked()
            {
                *act = Act::TogglePlay;
            }
            if ui
                .add_enabled(
                    self.queue.next_index().is_some(),
                    egui::Button::new(language.text("下一首", "Next")),
                )
                .clicked()
            {
                *act = Act::Next;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let effective = if self.muted { 0.0 } else { self.vol };
                let mut volume = effective;
                if screen.width() >= 620.0 {
                    ui.add(
                        egui::Slider::new(&mut volume, 0.0..=1.0)
                            .show_value(false)
                            .trailing_fill(true),
                    );
                }
                if volume != effective {
                    *act = Act::Volume(volume);
                }
                if ui
                    .button(if self.muted {
                        language.text("取消静音", "Unmute")
                    } else {
                        language.text("音量", "Volume")
                    })
                    .clicked()
                {
                    *act = Act::ToggleMute;
                }
            });
        });
    }
}
