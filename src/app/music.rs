use super::*;
use crate::queue::{Item, Source};

pub(super) const TABS_TOP: f32 = 64.0;
const CONTENT_TOP: f32 = TABS_TOP + design::CONTROL_HEIGHT + 16.0;

fn item_is_music(item: &Item) -> bool {
    match &item.source {
        Source::File(path) => !std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| {
                crate::media::VIDEO_EXTENSIONS.contains(&s.to_ascii_lowercase().as_str())
            }),
        Source::Torrent(_) => false,
    }
}

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
        self.persist_queue();
    }
    pub(super) fn is_music(&self) -> bool {
        self.media_info.as_ref().map_or_else(
            || {
                if self.loading {
                    // While a track is opening, keep the previous UI mode so the
                    // interface does not flash between music and video layouts.
                    if let Some(kind) = self.ui_kind_music {
                        return kind;
                    }
                }
                if self.player.is_none() {
                    self.mode == UiMode::Music
                } else {
                    self.source
                        .as_deref()
                        .is_some_and(crate::media_source::is_qq)
                }
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
        self.update_library();
        self.library.flush();
        if append && self.player.is_some() {
            for item in items {
                if item_is_music(&item) == self.queue_kind_music {
                    self.queue.items.push(item);
                } else {
                    self.queue_inactive.items.push(item);
                }
            }
            self.persist_queue();
            return;
        }
        let first_music = item_is_music(&items[0]);
        self.comic.flush();
        self.mode = if first_music {
            UiMode::Music
        } else {
            UiMode::Video
        };
        self.set_active_queue(first_music);
        if !append {
            self.queue.items.clear();
            self.queue.current = None;
            self.queue_inactive.items.clear();
            self.queue_inactive.current = None;
        }
        let start = self.queue.items.len();
        for item in items {
            if item_is_music(&item) == first_music {
                self.queue.items.push(item);
            } else {
                self.queue_inactive.items.push(item);
            }
        }
        if !append || self.player.is_none() {
            self.play_queue(start);
        }
        self.persist_queue();
    }
    pub(super) fn play_queue(&mut self, index: usize) {
        self.update_library();
        self.library.flush();
        let Some(item) = self.queue.select(index) else {
            return;
        };
        self.online_music.autoplay = false;
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
        self.persist_queue();
    }
    fn queue_contents(&mut self, ui: &mut Ui, act: &mut Act) {
        let language = self.settings.language;
        self.queue_drag.begin(ui);
        if self.queue.items.is_empty() {
            ui.label(language.text("队列为空", "Queue is empty"));
        }
        let mut rects: Vec<(usize, Rect)> = Vec::new();
        let scroll = design::scroll().auto_shrink([false, false]).show(ui, |ui| {
            for (index, item) in self.queue.items.iter().enumerate() {
                let current = self.queue.current == Some(index);
                let path = match &item.source {
                    Source::File(path) => Some(path.clone()),
                    Source::Torrent(_) => None,
                };
                if let Some(path) = &path {
                    self.metadata_cache.request(path, &self.metadata_requests);
                }
                let metadata = path
                    .as_ref()
                    .and_then(|path| self.metadata_cache.get(path))
                    .and_then(|value| value.as_ref())
                    .cloned();
                let title = metadata
                    .as_ref()
                    .and_then(|info| info.title.as_deref())
                    .unwrap_or(&item.title);
                let detail = metadata
                    .as_ref()
                    .map(|info| {
                        [info.artist.as_deref(), info.album.as_deref()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" · ")
                    })
                    .unwrap_or_default();
                ui.push_id(index, |ui| {
                    // The scroll area already reserves its own gutter; only
                    // subtract the card's padding from its content width.
                    let inner_width = (ui.available_width() - 16.0).max(80.0);
                    let row_rect =
                        Rect::from_min_size(ui.cursor().min, vec2(inner_width + 16.0, 48.0));
                    let row = self
                        .queue_drag
                        .row(ui, row_rect, index, &item.source)
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let out = egui::Frame::new()
                        .fill(if row.contains_pointer() {
                            if current {
                                design::SELECTED.gamma_multiply(1.1)
                            } else {
                                design::HOVER
                            }
                        } else if current {
                            design::SELECTED
                        } else {
                            SURFACE
                        })
                        .corner_radius(8)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.set_min_width(inner_width);
                            ui.set_max_width(inner_width);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let mut tex = None;
                                if let Some(path) = &path
                                    && let Some(art) =
                                        metadata.as_ref().and_then(|info| info.artwork.as_ref())
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
                                let playing =
                                    current && self.player.as_ref().is_some_and(|p| p.is_playing());
                                let (tile, resp) =
                                    ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                                let painter = ui.painter();
                                painter.rect_filled(
                                    tile,
                                    CornerRadius::same(4),
                                    design::BACKGROUND,
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
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.set_min_width(label_width);
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        ui.add(
                                            egui::Label::new(RichText::new(title).color(
                                                if current { Color32::WHITE } else { MUTED },
                                            ))
                                            .truncate()
                                            .selectable(false),
                                        );
                                        if !detail.is_empty() {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&detail).small().color(MUTED),
                                                )
                                                .truncate()
                                                .selectable(false),
                                            );
                                        }
                                    },
                                )
                                .response
                                .on_hover_text(format!(
                                    "{title}\n{detail}\n{}\n{}",
                                    path.as_deref().unwrap_or(&item.title),
                                    language.text("长按拖动排序", "Hold to reorder")
                                ));
                                if icon_button(ui, 22.0, 1.0, draw_trash)
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
        if let Some(drag) = self.queue_drag.finish(ui) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if !scroll.inner_rect.contains(drag.position) {
                return;
            }
            let target = rects
                .iter()
                .filter(|(_, rect)| drag.position.y > rect.center().y)
                .count();
            if let Some((_, anchor)) = rects.get(target).or_else(|| rects.last()) {
                let y = if target < rects.len() {
                    anchor.min.y - 2.0
                } else {
                    anchor.max.y + 2.0
                };
                ui.painter().with_clip_rect(scroll.inner_rect).line_segment(
                    [pos2(anchor.min.x, y), pos2(anchor.max.x, y)],
                    Stroke::new(2.0, ACCENT),
                );
            }
            if drag.released && target != drag.index && target != drag.index + 1 {
                self.queue.relocate(drag.index, target);
                self.persist_queue();
            }
        }
    }
    pub(super) fn queue_sidebar(&mut self, root: &mut Ui, rect: Rect, act: &mut Act) {
        root.painter().rect_filled(rect, 0, SURFACE);
        root.painter().line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, design::LINE),
        );
        let mut ui = root.new_child(
            UiBuilder::new()
                .id_salt("queue-sidebar")
                .max_rect(rect.shrink(12.0))
                .layout(Layout::top_down(Align::Min)),
        );
        ui.set_clip_rect(rect.shrink(1.0));
        ui.horizontal(|ui| {
            if icon_button(ui, 20.0, 1.0, draw_close)
                .on_hover_text(self.settings.language.text("收起队列", "Collapse queue"))
                .clicked()
            {
                self.queue_open = false;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, 24.0, 1.0, draw_add)
                    .on_hover_text(self.settings.language.text("添加文件", "Add files"))
                    .clicked()
                {
                    *act = Act::AddFiles;
                }
                if !self.is_music()
                    && icon_button(ui, 24.0, 1.0, draw_magnet)
                        .on_hover_text(self.settings.language.text("打开磁链", "Open magnet link"))
                        .clicked()
                {
                    self.magnet_open = true;
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
        _screen: Rect,
        playback: Rect,
        act: &mut Act,
    ) {
        let language = self.settings.language;
        let (_, _, position) = self
            .player
            .as_ref()
            .map(|p| (p.is_playing(), p.duration(), p.position()))
            .unwrap_or((false, 0.0, 0.0));
        let content = Rect::from_min_max(
            playback.min + vec2(40.0, CONTENT_TOP),
            playback.max - vec2(40.0, 24.0),
        );
        let short = content.height() < 280.0;
        let compact = short || content.width() < 560.0;
        let (left, lyrics_rect) = if !compact {
            let half = content.width() * 0.5;
            (
                Rect::from_min_max(
                    content.min,
                    pos2(content.min.x + half - 16.0, content.max.y),
                ),
                Some(Rect::from_min_max(
                    pos2(content.min.x + half + 16.0, content.min.y),
                    content.max,
                )),
            )
        } else if !short {
            // A narrow window or expanded queue uses a compact cover above lyrics.
            (
                Rect::from_min_size(content.min, vec2(content.width(), 88.0)),
                Some(Rect::from_min_max(
                    content.min + vec2(0.0, 104.0),
                    content.max,
                )),
            )
        } else {
            (content, None)
        };
        if self.player.is_some() || self.loading {
            let size = if compact {
                left.height().clamp(40.0, 110.0)
            } else {
                (left.height() - 112.0)
                    .clamp(90.0, 500.0)
                    .min(left.width() - 20.0)
            };
            let (cover, text_rect) = if compact {
                let text_height = (left.height() - 12.0).min(120.0);
                let cover = Rect::from_center_size(
                    pos2(left.min.x + size * 0.5, left.center().y),
                    Vec2::splat(size),
                );
                let text = Rect::from_min_max(
                    pos2(cover.max.x + 18.0, left.center().y - text_height * 0.5),
                    pos2(left.max.x, left.center().y + text_height * 0.5),
                );
                (cover, text)
            } else {
                let gap = 16.0;
                let text_height = 84.0;
                let total = size + gap + text_height;
                let top = left.center().y - total * 0.5;
                let cover = Rect::from_center_size(
                    pos2(left.center().x, top + size * 0.5),
                    Vec2::splat(size),
                );
                let text = Rect::from_min_max(
                    pos2(cover.min.x, top + size + gap),
                    pos2(cover.max.x, top + total),
                );
                (cover, text)
            };
            let painter = root.painter();
            painter.rect_filled(cover, 16, SURFACE);
            if let Some(art) = &self.artwork {
                design::cover(root, art, cover, 10);
            } else {
                painter.text(
                    cover.center(),
                    egui::Align2::CENTER_CENTER,
                    "♫",
                    egui::FontId::proportional(size * 0.4),
                    ACCENT,
                );
            }
            let mut info_ui = root.new_child(
                UiBuilder::new()
                    .id_salt("music-info")
                    .max_rect(text_rect)
                    .layout(Layout::top_down(Align::Min)),
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
            if compact
                && let Some(index) = self.lyrics.current_index(position)
                && let Some(line) = self.lyrics.lines.get(index)
            {
                info_ui.add(
                    egui::Label::new(RichText::new(&line.text).color(ACCENT))
                        .truncate()
                        .selectable(false),
                );
            }
        }
        if let Some(rect) = lyrics_rect {
            if self.workspace.listening_tab == 1 {
                let mut ui =
                    workspace::child(root, "track-details", rect.shrink2(vec2(36.0, 60.0)));
                ui.label(
                    RichText::new(language.text("歌曲信息", "Track information"))
                        .size(22.0)
                        .strong(),
                );
                ui.add_space(24.0);
                let info = self.media_info.as_ref();
                for (name, value) in [
                    (language.text("标题", "Title"), self.title.as_str()),
                    (
                        language.text("艺术家", "Artist"),
                        info.and_then(|i| i.artist.as_deref()).unwrap_or("—"),
                    ),
                    (
                        language.text("专辑", "Album"),
                        info.and_then(|i| i.album.as_deref()).unwrap_or("—"),
                    ),
                ] {
                    ui.label(RichText::new(name).size(12.0).color(MUTED));
                    ui.label(value);
                    ui.add_space(16.0);
                }
                if let Some(source) = &self.source {
                    ui.label(
                        RichText::new(language.text("文件", "File"))
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.add(egui::Label::new(source.to_string_lossy()).wrap());
                }
            } else if self.lyrics.lines.is_empty() {
                let mut ui = workspace::child(root, "no-lyrics", rect.shrink(28.0));
                ui.add_space(rect.height() * 0.32);
                ui.label(
                    RichText::new(
                        language.text("静静聆听，此刻的声音", "Let the music fill the moment"),
                    )
                    .size(24.0)
                    .color(design::TEXT),
                );
                ui.label(
                    RichText::new(language.text("暂无歌词", "No lyrics available")).color(MUTED),
                );
            } else {
                self.lyrics_panel(root, rect, position, act);
            }
        }
    }
    /// Time-synced lyric sheet: highlighted current line, click a line to seek,
    /// auto-scroll until the user scrolls away manually.
    fn lyrics_panel(&mut self, root: &mut Ui, rect: Rect, position: f64, act: &mut Act) {
        root.painter().rect_filled(rect, 12, BACKGROUND);
        let mut ui = root.new_child(
            UiBuilder::new()
                .id_salt("lyrics-panel")
                .max_rect(rect.shrink(10.0))
                .layout(Layout::top_down(Align::Min)),
        );
        ui.set_clip_rect(rect.intersect(root.clip_rect()));
        let current = self.lyrics.current_index(position);
        let synced = self.lyrics.synced;
        let mut follow = self.lyrics_follow;
        let mut seek = None;
        let height = rect.height();
        design::scroll()
            .id_salt("lyrics-scroll")
            .auto_shrink([false, false])
            .show(&mut ui, |ui| {
                ui.add_space((height * 0.3).max(8.0));
                for (index, line) in self.lyrics.lines.iter().enumerate() {
                    let active = current == Some(index);
                    let mut text = RichText::new(&line.text)
                        .size(if active { 26.0 } else { 22.0 })
                        .color(if active { design::TEXT } else { MUTED });
                    if active {
                        text = text.strong();
                    }
                    let mut response = ui.add(egui::Label::new(text).selectable(false));
                    if synced {
                        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            seek = Some(line.time);
                            // Jumping to a line resumes auto-scroll from there.
                            follow = true;
                        }
                    }
                    if active && follow {
                        ui.scroll_to_rect(response.rect, Some(Align::Center));
                    }
                    ui.add_space(22.0);
                }
                ui.add_space((height * 0.5).max(8.0));
            });
        if ui.input(|i| {
            i.smooth_scroll_delta().y.abs() > 0.5
                && i.pointer.hover_pos().is_some_and(|p| rect.contains(p))
        }) {
            follow = false;
        }
        self.lyrics_follow = follow;
        if let Some(time) = seek {
            *act = Act::Seek(time);
        }
    }
}
