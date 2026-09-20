use super::*;

impl App {
    pub(super) fn start_magnet(&mut self) {
        self.torrent_job = None;
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
                ui.label(language.text(
                    "粘贴磁链，解析后选择音乐或视频",
                    "Paste a magnet link, then choose music or video",
                ));
                ui.add(
                    egui::TextEdit::multiline(&mut self.magnet_input)
                        .hint_text("magnet:?xt=urn:btih:…")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.torrent_resolving,
                            egui::Button::new(language.text("解析磁链", "Resolve magnet")),
                        )
                        .clicked()
                    {
                        resolve = true;
                    }
                    if self.torrent_job.is_some()
                        && ui.button(language.text("停止任务", "Stop task")).clicked()
                    {
                        stop = true;
                    }
                });
                if self.torrent_resolving {
                    ui.spinner();
                }
                ui.label(&self.torrent_status);
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for file in &self.torrent_files {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button(language.text("播放", "Play")).clicked() {
                                    selected = Some(file.id);
                                }
                                ui.label(format!("{:.1} MiB", file.size as f64 / 1048576.0));
                                ui.add(egui::Label::new(&file.name).truncate())
                                    .on_hover_text(&file.name);
                            });
                        }
                    });
                if self.torrent_selected.is_some() {
                    let p = &self.torrent_progress;
                    ui.add(
                        egui::ProgressBar::new(p.downloaded as f32 / p.total.max(1) as f32)
                            .show_percentage(),
                    );
                    ui.label(format!(
                        "{:.1} / {:.1} MiB · {:.2} MiB/s",
                        p.downloaded as f64 / 1048576.0,
                        p.total as f64 / 1048576.0,
                        p.download_mbps
                    ));
                    if let Some(job) = &self.torrent_job
                        && ui
                            .button(if p.paused {
                                language.text("继续下载", "Resume download")
                            } else {
                                language.text("暂停下载", "Pause download")
                            })
                            .clicked()
                    {
                        job.pause(!p.paused);
                        if !p.paused
                            && let Some(player) = &self.player
                        {
                            player.set_playing(false);
                        }
                    }
                }
                ui.small(language.text(
                    "关闭任务保留缓存；BT 上传限速 256 KiB/s",
                    "Stopping keeps cached files; BT upload is limited to 256 KiB/s",
                ));
                ui.small(crate::torrent::cache_dir().display().to_string());
                ui.separator();
                ui.small(language.text(
                    "TUN 用户：BT 可单独直连，LLM 域名保留代理",
                    "TUN users: route BT directly while keeping the LLM endpoint proxied",
                ));
                if ui
                    .button(language.text("复制 Clash 分流规则", "Copy Clash routing rules"))
                    .clicked()
                {
                    let executable = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_else(|| "replayer.exe".into());
                    let base = if self.settings.llm.base_url.trim().is_empty() {
                        &self.llm_defaults.base_url
                    } else {
                        &self.settings.llm.base_url
                    };
                    ctx.copy_text(crate::torrent::routing_rules(&executable, base));
                }
                ui.small(language.text(
                    "仅复制规则，不自动修改代理；PROXY 需对应你的策略组",
                    "Copies rules only; replace PROXY with your actual policy group",
                ));
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
