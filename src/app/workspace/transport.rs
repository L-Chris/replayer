use super::*;

pub(super) const MUSIC_BAR_HEIGHT: f32 = 100.0;

impl App {
    pub(super) fn mini_transport(&mut self, root: &mut Ui, screen: Rect, act: &mut Act) {
        let l = self.settings.language;
        let wide = screen.width() >= 1100.0;
        let bar = Rect::from_min_max(
            pos2(screen.min.x, screen.max.y - MUSIC_BAR_HEIGHT),
            screen.max,
        );
        root.painter().rect_filled(bar, 0, SURFACE);
        root.painter()
            .line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, LINE));
        let left_w = (screen.width() * 0.26).clamp(200.0, 340.0);
        let cover = Rect::from_min_size(bar.min + vec2(20.0, 18.0), Vec2::splat(64.0));
        root.painter().rect_filled(cover, 7, BACKGROUND);
        let response = root.interact(cover, Id::new("mini-cover"), Sense::click());
        if self.player.is_some() {
            design::card_hover(root, &response, 7);
        }
        if let Some(art) = &self.artwork {
            design::cover(root, art, cover.shrink(3.0), 5);
        } else {
            draw_music(root.painter(), cover.shrink(19.0), MUTED);
        }
        if response
            .on_hover_text(l.text("展开播放页", "Now playing"))
            .clicked()
            && self.player.is_some()
        {
            self.workspace.page = if self.is_music() {
                Page::Listening
            } else {
                Page::Playing
            };
        }
        let artist = self
            .media_info
            .as_ref()
            .and_then(|i| i.artist.as_deref())
            .unwrap_or("");
        let track_label = if self.title.is_empty() {
            l.text("尚未播放", "Nothing playing").to_owned()
        } else if artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.title, artist)
        };
        let mut metadata = child(
            root,
            "mini-track",
            Rect::from_min_max(
                bar.min + vec2(98.0, 28.0),
                pos2(
                    (bar.center().x - 130.0).min(bar.min.x + 450.0),
                    bar.min.y + 52.0,
                ),
            ),
        );
        metadata
            .add(egui::Label::new(RichText::new(&track_label).size(14.0).strong()).truncate())
            .on_hover_text(&track_label);
        let path = self
            .source
            .as_ref()
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned());
        let selected = path.as_ref().is_some_and(|path| {
            self.library
                .records
                .iter()
                .any(|r| r.path == *path && r.favorite)
        });
        if path.is_some() {
            if design::favorite_at(
                root,
                Id::new("mini-favorite"),
                Rect::from_min_size(bar.min + vec2(96.0, 52.0), Vec2::splat(32.0)),
                selected,
            )
            .on_hover_text(l.text("收藏歌曲", "Favorite track"))
            .clicked()
                && let Some(path) = &path
            {
                self.library
                    .toggle_favorite(path, &self.title, self.is_music());
            }
        } else {
            design::heart(
                root.painter(),
                Rect::from_min_size(bar.min + vec2(102.0, 58.0), Vec2::splat(20.0)),
                MUTED,
                false,
            );
        }
        let playing = self.player.as_ref().is_some_and(|p| p.is_playing());
        let (position, duration) = self
            .player
            .as_ref()
            .map(|p| (p.position(), p.duration()))
            .unwrap_or_default();
        let center = pos2(bar.center().x, bar.min.y + 34.0);
        let mut shuffle_ui = control_slot(root, "music-shuffle", center + vec2(-100.0, 0.0), 24.0);
        if icon_button(&mut shuffle_ui, 24.0, 1.0, |p, r, c| {
            draw_shuffle(p, r, if self.shuffle { ACCENT } else { c })
        })
        .on_hover_text(l.text("随机播放", "Shuffle"))
        .clicked()
        {
            self.shuffle = !self.shuffle;
        }
        let mut previous_ui = control_slot(root, "music-previous", center + vec2(-60.0, 0.0), 28.0);
        if self.player.is_none() {
            previous_ui.disable();
        }
        if icon_button(&mut previous_ui, 28.0, 1.0, draw_prev)
            .on_hover_text(l.text("上一首", "Previous"))
            .clicked()
        {
            *act = Act::Previous;
        }
        let mut play_ui = control_slot(root, "music-play", center, 46.0);
        if self.player.is_none() {
            play_ui.disable();
        }
        if play_circle_button(&mut play_ui, 46.0, playing)
            .on_hover_text(l.text("播放 / 暂停", "Play / pause"))
            .clicked()
        {
            *act = Act::TogglePlay;
        }
        let mut next_ui = control_slot(root, "music-next", center + vec2(60.0, 0.0), 28.0);
        if self.player.is_none() {
            next_ui.disable();
        }
        if icon_button(&mut next_ui, 28.0, 1.0, draw_next)
            .on_hover_text(l.text("下一首", "Next"))
            .clicked()
        {
            *act = Act::Next;
        }
        let mut repeat_ui = control_slot(root, "music-repeat", center + vec2(100.0, 0.0), 24.0);
        if icon_button(&mut repeat_ui, 24.0, 1.0, |p, r, c| {
            draw_repeat(
                p,
                r,
                if self.loop_single { ACCENT } else { c },
                self.loop_single,
            )
        })
        .on_hover_text(l.text("单曲循环", "Repeat one"))
        .clicked()
        {
            self.loop_single = !self.loop_single;
        }
        // The primary transport stays on the window axis independently of metadata.
        let seek_width = (screen.width() - 2.0 * (left_w + 16.0)).clamp(290.0, 560.0);
        let seek_rect = Rect::from_center_size(
            pos2(bar.center().x, bar.max.y - 24.0),
            vec2(seek_width, 24.0),
        );
        root.painter().text(
            seek_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            fmt_time(if self.scrubbing { self.scrub } else { position }),
            egui::FontId::proportional(11.0),
            MUTED,
        );
        root.painter().text(
            seek_rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            fmt_time(duration),
            egui::FontId::proportional(11.0),
            MUTED,
        );
        let mut seek = child(root, "mini-seek", seek_rect.shrink2(vec2(46.0, 0.0)));
        self.seek_control(&mut seek, seek_width - 92.0, duration, position, 1.0, act);
        let cy = bar.center().y;
        let mut queue = control_slot(root, "mini-queue", pos2(bar.max.x - 34.0, cy), 28.0);
        if icon_button(&mut queue, 28.0, 1.0, |p, r, c| {
            draw_queue(p, r, if self.queue_open { ACCENT } else { c })
        })
        .on_hover_text(l.text("\u{64ad}\u{653e}\u{961f}\u{5217}", "Play queue"))
        .clicked()
        {
            self.queue_open = !self.queue_open;
        }
        root.painter().line_segment(
            [
                pos2(bar.max.x - 62.0, cy - 14.0),
                pos2(bar.max.x - 62.0, cy + 14.0),
            ],
            Stroke::new(1.0, LINE),
        );
        self.volume_control(root, screen, pos2(bar.max.x - 90.0, cy), 1.0, act);
        if wide {
            root.painter().line_segment(
                [
                    pos2(bar.max.x - 118.0, cy - 14.0),
                    pos2(bar.max.x - 118.0, cy + 14.0),
                ],
                Stroke::new(1.0, LINE),
            );
            let mut lyrics = control_slot(root, "mini-lyrics", pos2(bar.max.x - 146.0, cy), 28.0);
            if icon_button(&mut lyrics, 28.0, 1.0, draw_subtitles)
                .on_hover_text(l.text("\u{5c55}\u{5f00}\u{6b4c}\u{8bcd}", "Show lyrics"))
                .clicked()
                && self.is_music()
            {
                self.workspace.listening_tab = 0;
                self.workspace.page = Page::Listening;
            }
        }
    }

    pub(in crate::app) fn video_transport(
        &mut self,
        root: &mut Ui,
        screen: Rect,
        alpha: f32,
        act: &mut Act,
    ) {
        let l = self.settings.language;
        let wide = screen.width() >= 1100.0;
        let bar = Rect::from_min_max(pos2(screen.min.x, screen.max.y - BAR_BOTTOM), screen.max);
        let cy = bar.min.y + 40.0;
        root.painter()
            .rect_filled(bar, 0, SURFACE.gamma_multiply(alpha * 0.96));
        root.painter().line_segment(
            [bar.left_top(), bar.right_top()],
            Stroke::new(1.0, LINE.gamma_multiply(alpha)),
        );
        let (pos, dur, playing) = self
            .player
            .as_ref()
            .map(|p| (p.position(), p.duration(), p.is_playing()))
            .unwrap_or_default();
        for (index, offset, size) in [(0, 42.0, 36.0), (1, 93.0, 38.0), (2, 144.0, 36.0)] {
            let mut ui = control_slot(
                root,
                &format!("video-control-{index}"),
                pos2(bar.min.x + offset, cy),
                size,
            );
            let response = icon_button(&mut ui, size, alpha, |p, r, c| match index {
                0 => draw_seek_ten(p, r, c, false),
                1 if playing => draw_pause(p, r, c),
                1 => draw_play(p, r, c),
                _ => draw_seek_ten(p, r, c, true),
            });
            let label = match index {
                0 => l.text("后退 10 秒", "Back 10 seconds"),
                1 => l.text("播放 / 暂停", "Play / pause"),
                _ => l.text("前进 10 秒", "Forward 10 seconds"),
            };
            if response.on_hover_text(label).clicked() {
                *act = match index {
                    0 => Act::Seek((pos - 10.0).max(0.0)),
                    1 => Act::TogglePlay,
                    _ => Act::Seek((pos + 10.0).min(dur)),
                };
            }
        }
        let time = format!(
            "{} / {}",
            fmt_time(if self.scrubbing { self.scrub } else { pos }),
            fmt_time(dur)
        );
        root.painter().text(
            pos2(bar.min.x + 178.0, cy),
            egui::Align2::LEFT_CENTER,
            time,
            egui::FontId::proportional(12.0),
            design::TEXT.gamma_multiply(alpha),
        );
        let mut x = bar.max.x - 24.0;
        let mut full = control_slot(root, "video-fullscreen", pos2(x - 15.0, cy), 30.0);
        if icon_button(&mut full, 30.0, alpha, draw_fullscreen)
            .on_hover_text(l.text("全屏 / 退出全屏", "Toggle fullscreen"))
            .clicked()
        {
            *act = Act::ToggleFullscreen;
        }
        x -= 40.0;
        let mut queue = control_slot(root, "video-queue", pos2(x - 14.0, cy), 28.0);
        if icon_button(&mut queue, 28.0, alpha, |p, r, c| {
            draw_queue(p, r, if self.queue_open { ACCENT } else { c })
        })
        .on_hover_text(l.text("播放队列", "Play queue"))
        .clicked()
        {
            self.queue_open = !self.queue_open;
        }
        x -= 38.0;
        let mut settings = control_slot(root, "video-settings", pos2(x - 13.0, cy), 26.0);
        self.settings_button(&mut settings, alpha);
        x -= 36.0;
        if wide {
            let chip = Rect::from_center_size(pos2(x - 56.0, cy), vec2(112.0, 36.0));
            root.painter()
                .rect_filled(chip, 18, BACKGROUND.gamma_multiply(alpha));
            root.painter().rect_stroke(
                chip,
                18,
                Stroke::new(1.0, LINE.gamma_multiply(alpha)),
                egui::StrokeKind::Inside,
            );
            let active = self.subtitle_job.is_some();
            let label = if active {
                l.text("● AI 字幕 · 生成中", "● AI · generating")
            } else if self.subtitles.is_empty() {
                l.text("AI 字幕", "AI subtitles")
            } else {
                l.text("● AI 字幕 · 就绪", "● AI · ready")
            };
            root.painter().text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                if active { ACCENT } else { MUTED }.gamma_multiply(alpha),
            );
            x -= 122.0;
        }
        let caption_width = if wide {
            root.painter()
                .layout_no_wrap(
                    l.text("字幕", "Subtitles").into(),
                    egui::FontId::proportional(12.0),
                    design::TEXT,
                )
                .size()
                .x
                + 32.0
        } else {
            26.0
        };
        let mut captions = child(
            root,
            "video-subtitles",
            Rect::from_min_size(
                pos2(x - caption_width, cy - 13.0),
                vec2(caption_width, 26.0),
            ),
        );
        captions.spacing_mut().interact_size.y = 26.0;
        captions.spacing_mut().item_spacing.x = 6.0;
        captions.horizontal(|ui| {
            self.subtitle_menu(ui, alpha, act);
            if wide {
                ui.label(RichText::new(l.text("字幕", "Subtitles")).size(12.0));
            }
        });
        x -= caption_width + 10.0;
        self.volume_control(root, screen, pos2(x - 14.0, cy), alpha, act);
        x -= 38.0;
        let seek_rect = Rect::from_min_max(
            pos2(bar.min.x + 290.0, cy - 12.0),
            pos2(x - 10.0, cy + 12.0),
        );
        let mut seek = child(root, "video-seek", seek_rect);
        self.seek_control(&mut seek, seek_rect.width().max(30.0), dur, pos, alpha, act);
    }

    fn seek_control(
        &mut self,
        ui: &mut Ui,
        width: f32,
        duration: f64,
        position: f64,
        alpha: f32,
        act: &mut Act,
    ) {
        let shown = if self.scrubbing { self.scrub } else { position };
        let (rect, response) = hslider(
            ui,
            width,
            24.0,
            (shown / duration.max(0.001)).clamp(0.0, 1.0) as f32,
            3.0,
            true,
            alpha,
        );
        if duration <= 0.0 {
            return;
        }
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.scrubbing = true;
            self.scrub = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64 * duration;
        }
        if response.drag_stopped() {
            *act = Act::Seek(self.scrub);
            self.scrubbing = false;
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            *act = Act::Seek(((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64 * duration);
        }
    }

    fn volume_control(
        &mut self,
        root: &mut Ui,
        screen: Rect,
        center: egui::Pos2,
        alpha: f32,
        act: &mut Act,
    ) {
        let mut ui = control_slot(root, "transport-volume", center, 28.0);
        let response = icon_button(&mut ui, 28.0, alpha, |p, r, c| {
            draw_volume(p, r, c, self.muted, self.vol)
        })
        .on_hover_text(self.settings.language.text("\u{97f3}\u{91cf}", "Volume"));
        if response.clicked() {
            self.volume_open = !self.volume_open;
        }
        self.volume_popup(root, screen, response.rect, act);
    }
}

fn control_slot(root: &mut Ui, id: &str, center: egui::Pos2, size: f32) -> Ui {
    child(root, id, Rect::from_center_size(center, Vec2::splat(size)))
}
