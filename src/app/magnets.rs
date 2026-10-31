use super::*;

impl App {
    pub(super) fn start_magnet(&mut self) {
        self.update_library();
        self.online_music.cancel_playback();
        self.online_origin = None;
        self.workspace.resume = None;
        self.torrent_job = None;
        self.loading = false;
        self.set_active_queue(false);
        self.queue = Default::default();
        self.media_info = None;
        self.artwork = None;
        self.torrent_files.clear();
        self.torrent_selected = None;
        self.torrent_progress = Default::default();
        self.subtitle_job = None;
        self.subtitles.clear();
        self.subtitle_status.clear();
        self.subtitle_skipped.clear();
        self.subtitle_metrics.clear();
        self.player = None;
        self.video.clear();
        self.source = None;
        self.title.clear();
        self.error = None;
        self.magnet_open = true;
        match crate::torrent::Job::start(
            self.magnet_input.trim().to_owned(),
            crate::torrent::cache_dir(),
        ) {
            Ok(job) => {
                self.torrent_job = Some(job);
                self.torrent_resolving = true;
                self.torrent_status = self
                    .settings
                    .language
                    .text("正在获取磁链文件列表…", "Fetching magnet metadata…")
                    .into();
            }
            Err(error) => {
                self.torrent_resolving = false;
                self.torrent_status = error.to_string();
            }
        }
    }
    pub(super) fn poll_torrent(&mut self) {
        let events = self
            .torrent_job
            .as_ref()
            .map(|job| job.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                crate::torrent::Event::Files(files) => {
                    self.set_active_queue(false);
                    self.queue.items = files
                        .iter()
                        .map(|file| crate::queue::Item {
                            source: crate::queue::Source::Torrent(file.id),
                            title: file.name.clone(),
                        })
                        .collect();
                    self.queue.current = None;
                    self.torrent_files = files;
                    self.torrent_resolving = false;
                    self.torrent_status = self
                        .settings
                        .language
                        .text("选择要播放的音乐或视频", "Choose music or video to play")
                        .into();
                }
                crate::torrent::Event::Ready { url, file } => {
                    self.load_media(url, true);
                    self.title = file.name.clone();
                    self.torrent_selected = Some(file);
                    self.torrent_status = self
                        .settings
                        .language
                        .text("正在播放所选媒体", "Playing selected media")
                        .into();
                    self.magnet_open = false;
                }
                crate::torrent::Event::Progress(progress) => {
                    self.torrent_progress = progress;
                    if self.torrent_progress.finished {
                        self.torrent_status = self
                            .settings
                            .language
                            .text("所选媒体已下载完成", "Selected media download complete")
                            .into();
                    }
                }
                crate::torrent::Event::Error(error) => {
                    self.torrent_resolving = false;
                    self.torrent_status = error;
                    self.magnet_open = true;
                }
            }
        }
    }
    pub(super) fn magnet_dialog(&mut self, ctx: &egui::Context) {
        if !self.magnet_open {
            return;
        }
        let language = self.settings.language;
        let mut open = true;
        let mut resolve = false;
        let mut stop = false;
        let mut selected = None;
        egui::Window::new(language.text("磁链播放", "Magnet playback"))
            .id(Id::new("magnet-dialog"))
            .open(&mut open)
            .collapsible(false)
            .default_width((ctx.content_rect().width() - 60.0).clamp(280.0, 620.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                design::multiline_field(
                    ui,
                    &mut self.magnet_input,
                    ui.available_width(),
                    3,
                    "magnet:?xt=urn:btih:…",
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.add_enabled_ui(!self.torrent_resolving, |ui| {
                        if design::control(
                            ui,
                            language.text("解析磁链", "Resolve"),
                            None,
                            design::Control::Primary,
                            0.0,
                        )
                        .clicked()
                        {
                            resolve = true;
                        }
                    });
                    if self.torrent_job.is_some()
                        && design::icon_button(
                            ui,
                            icons::STOP,
                            language.text("停止任务", "Stop task"),
                            false,
                        )
                        .clicked()
                    {
                        stop = true;
                    }
                    if self.torrent_resolving {
                        ui.spinner();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let info_tip = format!(
                            "{}\n{}\n{}",
                            language.text(
                                "停止任务保留缓存；BT 上传限速 256 KiB/s",
                                "Stopping retains cached files; BT upload limit 256 KiB/s",
                            ),
                            language.text(
                                "TUN 开启时可复制 Clash 规则，将 PROXY 替换为实际策略组",
                                "When TUN is enabled, copy Clash rules and replace PROXY with your policy group",
                            ),
                            crate::torrent::cache_dir().display()
                        );
                        design::compact_icon_button(ui, icons::INFO, &info_tip, false);
                        if design::compact_icon_button(
                            ui,
                            icons::COPY,
                            language.text("复制 Clash 分流规则", "Copy Clash routing rules"),
                            false,
                        )
                        .clicked()
                        {
                            let executable = std::env::current_exe()
                                .ok()
                                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                                .unwrap_or_else(|| "pordo.exe".into());
                            let base = self
                                .settings
                                .ai
                                .providers
                                .iter()
                                .find(|p| p.id == self.settings.llm.selected.provider)
                                .map_or("", |p| p.base_url.as_str());
                            ctx.copy_text(crate::torrent::routing_rules(&executable, base));
                        }
                    });
                });
                if !self.torrent_status.is_empty() {
                    ui.small(RichText::new(&self.torrent_status).color(MUTED));
                }
                if !self.torrent_files.is_empty() {
                    design::scroll().max_height(200.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for file in &self.torrent_files {
                            let (row, row_resp) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 40.0),
                                Sense::click(),
                            );
                            if row_resp.hovered() {
                                ui.painter().rect_filled(row, 8, SURFACE);
                            }
                            let mut row_ui = workspace::child(
                                ui,
                                ("torrent-file", file.id),
                                row.shrink2(vec2(10.0, 5.0)),
                            );
                            row_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if design::compact_icon_button(
                                    ui,
                                    icons::PLAY,
                                    language.text("播放", "Play"),
                                    false,
                                )
                                .clicked()
                                    || row_resp.double_clicked()
                                {
                                    selected = Some(file.id);
                                }
                                ui.small(
                                    RichText::new(format!(
                                        "{:.1} MiB",
                                        file.size as f64 / 1048576.0
                                    ))
                                    .color(MUTED),
                                );
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    ui.add(egui::Label::new(&file.name).truncate())
                                        .on_hover_text(&file.name);
                                });
                            });
                        }
                    });
                }
                if self.torrent_selected.is_some() {
                    let p = &self.torrent_progress;
                    ui.add(
                        egui::ProgressBar::new(p.downloaded as f32 / p.total.max(1) as f32)
                            .show_percentage(),
                    );
                    ui.horizontal(|ui| {
                        ui.small(
                            RichText::new(format!(
                                "{:.1} / {:.1} MiB · {:.2} MiB/s",
                                p.downloaded as f64 / 1048576.0,
                                p.total as f64 / 1048576.0,
                                p.download_mbps
                            ))
                            .color(MUTED),
                        );
                        if let Some(job) = &self.torrent_job {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if design::compact_icon_button(
                                    ui,
                                    if p.paused { icons::PLAY } else { icons::PAUSE },
                                    if p.paused {
                                        language.text("继续下载", "Resume download")
                                    } else {
                                        language.text("暂停下载", "Pause download")
                                    },
                                    false,
                                )
                                .clicked()
                                {
                                    job.pause(!p.paused);
                                    if !p.paused
                                        && let Some(player) = &self.player
                                    {
                                        player.set_playing(false);
                                    }
                                }
                            });
                        }
                    });
                }
            });
        self.magnet_open = open;
        if resolve {
            self.start_magnet();
        }
        if stop {
            self.torrent_job = None;
            self.remove_torrent_queue();
            self.media_info = None;
            self.artwork = None;
            self.torrent_selected = None;
            self.torrent_files.clear();
            self.torrent_resolving = false;
            self.torrent_status = language
                .text("任务已停止，缓存已保留", "Stopped; cached files retained")
                .into();
            self.player = None;
            self.video.clear();
            self.subtitle_job = None;
            self.subtitles.clear();
            self.subtitle_status.clear();
            self.subtitle_skipped.clear();
            self.subtitle_metrics.clear();
            self.source = None;
        }
        if selected.is_some() {
            self.set_active_queue(false);
        }
        if let Some(id) = selected
            && let Some(job) = &self.torrent_job
        {
            self.queue.current = self
                .queue
                .items
                .iter()
                .position(|item| item.source == crate::queue::Source::Torrent(id));
            self.media_info = None;
            self.artwork = None;
            self.player = None;
            self.video.clear();
            self.subtitle_job = None;
            self.subtitles.clear();
            self.subtitle_status.clear();
            self.subtitle_skipped.clear();
            self.subtitle_metrics.clear();
            self.torrent_progress = Default::default();
            self.torrent_status = language.text("正在准备播放…", "Preparing playback…").into();
            job.select(id);
        }
    }
}
