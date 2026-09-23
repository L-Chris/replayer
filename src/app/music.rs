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
    fn queue_contents(&mut self, ui: &mut Ui, act: &mut Act) {
        let language = self.settings.language;
        if self.queue.items.is_empty() {
            ui.label(language.text("队列为空", "Queue is empty"));
        }
        let mut rects: Vec<(usize, Rect)> = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, item) in self.queue.items.iter().enumerate() {
                    let current = self.queue.current == Some(index);
                    let path = match &item.source {
                        Source::File(path) => Some(path.clone()),
                        Source::Torrent(_) => None,
                    };
                    if let Some(path) = &path
                        && !self.art_cache.contains_key(path)
                    {
                        self.art_cache.insert(path.clone(), None);
                        let _ = self.art_requests.send(path.clone());
                    }
                    ui.push_id(index, |ui| {
                        let out = egui::Frame::new()
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
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let mut tex = None;
                                    if let Some(path) = &path
                                        && let Some(Some(art)) = self.art_cache.get(path)
                                    {
                                        let ctx = ui.ctx().clone();
                                        let art = art.clone();
                                        tex = Some(
                                            self.art_textures
                                                .entry(path.clone())
                                                .or_insert_with(|| {
                                                    ctx.load_texture(
                                                        format!("queue-art-{index}"),
                                                        egui::ColorImage::from_rgba_unmultiplied(
                                                            [art.width, art.height],
                                                            &art.rgba,
                                                        ),
                                                        egui::TextureOptions::LINEAR,
                                                    )
                                                })
                                                .clone(),
                                        );
                                    }
                                    let playing = current
                                        && self.player.as_ref().is_some_and(|p| p.is_playing());
                                    let (tile, resp) =
                                        ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                                    let painter = ui.painter();
                                    painter.rect_filled(
                                        tile,
                                        CornerRadius::same(4),
                                        Color32::from_rgb(34, 41, 55),
                                    );
                                    if let Some(tex) = &tex {
                                        painter.image(
                                            tex.id(),
                                            tile,
                                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                            Color32::WHITE,
                                        );
                                        painter.rect_filled(
                                            tile,
                                            CornerRadius::same(4),
                                            Color32::from_black_alpha(if resp.hovered() {
                                                70
                                            } else {
                                                110
                                            }),
                                        );
                                    }
                                    if playing {
                                        draw_pause(painter, tile.shrink(9.0), Color32::WHITE);
                                    } else {
                                        draw_play(painter, tile.shrink(9.0), Color32::WHITE);
                                    }
                                    let tip = if current {
                                        if playing {
                                            language.text("暂停", "Pause")
                                        } else {
                                            language.text("播放", "Play")
                                        }
                                    } else {
                                        language.text("播放此项", "Play item")
                                    };
                                    if resp.on_hover_text(tip).clicked() {
                                        *act = if current {
                                            Act::TogglePlay
                                        } else {
                                            Act::QueuePlay(index)
                                        };
                                    }
                                    let label_width = (ui.available_width() - 40.0).max(40.0);
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
                        rects.push((index, out.response.rect));
                    });
                    ui.add_space(4.0);
                }
            });
        let (pressed, released, hover) = ui.input(|i| {
            (
                i.pointer.any_pressed(),
                i.pointer.any_released(),
                i.pointer.hover_pos(),
            )
        });
        if pressed
            && self.queue_drag.is_none()
            && let Some(pointer) = hover
        {
            self.queue_drag = rects
                .iter()
                .find(|(_, rect)| rect.contains(pointer))
                .map(|(index, _)| *index);
        }
        if let Some(drag) = self.queue_drag {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let target = hover.map_or(drag, |pointer| {
                rects
                    .iter()
                    .filter(|(_, rect)| pointer.y > rect.center().y)
                    .count()
            });
            if let Some((_, anchor)) = rects.get(target).or_else(|| rects.last()) {
                let y = if target < rects.len() {
                    anchor.min.y - 2.0
                } else {
                    anchor.max.y + 2.0
                };
                ui.painter().line_segment(
                    [pos2(anchor.min.x, y), pos2(anchor.max.x, y)],
                    Stroke::new(2.0, ACCENT),
                );
            }
            if released {
                if target != drag && target != drag + 1 {
                    self.queue.relocate(drag, target);
                }
                self.queue_drag = None;
            }
        }
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
                if icon_button(ui, 24.0, 1.0, draw_add)
                    .on_hover_text(self.settings.language.text("添加文件", "Add files"))
                    .clicked()
                {
                    *act = Act::AddFiles;
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
        let row = ui.available_rect_before_wrap();
        let row = Rect::from_min_max(row.min, pos2(row.max.x, row.min.y + 46.0));
        ui.allocate_rect(row, Sense::hover());
        let has_prev = self.queue.previous_index().is_some() || position > 3.0;
        let has_next = self.queue.next_index().is_some();
        let cluster = 26.0 + 16.0 + 30.0 + 16.0 + 46.0 + 16.0 + 30.0 + 16.0 + 26.0;
        let mut transport = root.new_child(
            UiBuilder::new()
                .id_salt("music-transport")
                .max_rect(Rect::from_center_size(
                    pos2(row.center().x, row.center().y),
                    vec2(cluster, row.height()),
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        transport.spacing_mut().item_spacing.x = 16.0;
        let single = self.loop_single;
        let mode = icon_button(&mut transport, 26.0, 1.0, |p, r, c| {
            draw_repeat(p, r, if single { ACCENT } else { c }, single)
        });
        if mode
            .on_hover_text(if single {
                language.text("单曲循环", "Repeat one")
            } else {
                language.text("列表循环", "Repeat list")
            })
            .clicked()
        {
            self.loop_single = !single;
        }
        if has_prev {
            let prev = icon_button(&mut transport, 30.0, 1.0, draw_prev);
            if prev
                .on_hover_text(language.text("上一首", "Previous"))
                .clicked()
            {
                *act = Act::Previous;
            }
        } else {
            disabled_icon_button(&mut transport, 30.0, draw_prev);
        }
        let play = play_circle_button(&mut transport, 46.0, playing);
        let play = play.on_hover_text(if playing {
            language.text("暂停", "Pause")
        } else {
            language.text("播放", "Play")
        });
        if play.clicked() {
            *act = Act::TogglePlay;
        }
        if has_next {
            let next = icon_button(&mut transport, 30.0, 1.0, draw_next);
            if next
                .on_hover_text(language.text("下一首", "Next"))
                .clicked()
            {
                *act = Act::Next;
            }
        } else {
            disabled_icon_button(&mut transport, 30.0, draw_next);
        }
        let effective = if self.muted { 0.0 } else { self.vol };
        let volume = icon_button(&mut transport, 26.0, 1.0, |p, r, c| {
            draw_volume(p, r, c, self.muted, effective)
        });
        let volume = volume.on_hover_text(language.text("音量", "Volume"));
        let volume_rect = volume.rect;
        if volume.clicked() {
            self.volume_open = !self.volume_open;
        }
        let mut queue_ui = root.new_child(
            UiBuilder::new()
                .id_salt("music-queue")
                .max_rect(Rect::from_min_max(
                    pos2(row.max.x - 26.0, row.min.y),
                    pos2(row.max.x, row.max.y),
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let queue = icon_button(&mut queue_ui, 26.0, 1.0, draw_queue);
        if queue
            .on_hover_text(language.text("播放队列", "Play queue"))
            .clicked()
        {
            self.queue_open = !self.queue_open;
        }
        if self.volume_open {
            let bottom = volume_rect.min.y - 10.0;
            let top_limit = screen.min.y + BAR_TOP + 4.0;
            let popup_h = (bottom - top_limit).clamp(60.0, 132.0);
            let popup_w = 40.0;
            let px = (volume_rect.center().x - popup_w * 0.5)
                .clamp(screen.min.x + 4.0, screen.max.x - popup_w - 4.0);
            let popup = Rect::from_min_max(pos2(px, bottom - popup_h), pos2(px + popup_w, bottom));
            let outside = root.ctx().input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .hover_pos()
                        .is_some_and(|p| !popup.contains(p) && !volume_rect.contains(p))
            });
            if outside {
                self.volume_open = false;
            } else {
                let painter = root.painter();
                painter.rect_filled(popup, CornerRadius::same(10), SURFACE);
                painter.rect_stroke(
                    popup,
                    CornerRadius::same(10),
                    Stroke::new(1.0, Color32::from_rgb(48, 57, 73)),
                    egui::StrokeKind::Outside,
                );
                let mut pui = root.new_child(
                    UiBuilder::new()
                        .id_salt("music-volume-popup")
                        .max_rect(popup.shrink(12.0))
                        .layout(Layout::top_down(Align::Center)),
                );
                let (vrect, vresp) =
                    vslider(&mut pui, 16.0, popup.height() - 24.0, effective, 3.5, 1.0);
                if (vresp.dragged() || vresp.clicked())
                    && let Some(pointer) = pui.input(|i| i.pointer.hover_pos())
                {
                    let f = ((vrect.max.y - pointer.y) / vrect.height()).clamp(0.0, 1.0);
                    *act = Act::Volume(f);
                }
            }
        }
    }
}
