use super::*;
use crate::music_sources::{self, Job, MusicSource, SearchPage, Song};
use std::sync::atomic::Ordering;

fn music_error(language: Language, error: &anyhow::Error) -> String {
    let message = error.to_string();
    if language == Language::English {
        return message;
    }
    let translated = match message.as_str() {
        "Set an API key in Music settings" => "请在音乐相关设置中填写 API Key",
        "Music API connection failed or timed out" => "音乐接口连接失败或超时，请检查网络",
        "Music API response interrupted" => "音乐接口响应中断，请重试",
        "Music API response too large" => "音乐接口响应超过大小限制",
        "Invalid music API response" | "Invalid music search response" => {
            "音乐源返回的数据格式不正确"
        }
        "No playable URL at this quality; try another quality" => {
            "该音质没有可播放地址，请尝试其他音质"
        }
        "Unsupported music download URL" => "音乐源返回了不支持的下载地址",
        "Unsupported audio format" => "音乐源返回了不支持的音频格式",
        "Invalid music content range"
        | "Invalid music range length"
        | "Invalid music response length" => "音乐源返回的数据范围不正确，请重试",
        "Music server changed or stopped supporting ranges" => {
            "音乐地址已失效或不再支持分段读取，请重试"
        }
        "Music download connection failed or timed out" => "音乐下载连接失败或超时",
        "Music download interrupted or timed out" => "音乐下载中断或超时，请重试",
        "Incomplete music download" => "音乐下载不完整，请重试",
        "Downloaded file is not playable audio" | "Downloaded file has no audio track" => {
            "下载的文件不是有效音频"
        }
        "Music file exceeds 512 MiB" => "音乐文件超过 512 MiB 上限",
        "Cannot create music directory"
        | "Cannot create music download"
        | "Cannot write music download" => "无法写入下载目录，请检查路径、权限和剩余空间",
        "Cannot save music metadata" => "无法保存歌曲元数据，请检查下载目录权限和剩余空间",
        "Cannot publish downloaded music file" => {
            "无法保存完整音乐文件，请确认下载目录支持硬链接且没有同名文件冲突"
        }
        _ => "",
    };
    if !translated.is_empty() {
        return translated.into();
    }
    if let Some(code) = message
        .strip_prefix("Music API HTTP ")
        .or_else(|| message.strip_prefix("Music API code "))
    {
        let detail = match code {
            "401" => "密钥无效",
            "403" => "无权限或额度受限",
            "429" => "请求过于频繁",
            "503" => "音乐源服务暂时不可用",
            _ => "请求失败",
        };
        return format!("音乐接口 {code}：{detail}");
    }
    if let Some(code) = message.strip_prefix("Music download HTTP ") {
        return format!("音乐下载失败：HTTP {code}");
    }
    message
}

pub(super) struct State {
    pub open: bool,
    keyword: String,
    searched: String,
    page: usize,
    next: bool,
    songs: Vec<Song>,
    search: Option<Job<SearchPage>>,
    download: Option<Job<PathBuf>>,
    downloading: Option<String>,
    status: String,
    failed: bool,
    pub preparing: Option<Job<music_sources::streaming::Prepared>>,
    pub stream: Option<music_sources::streaming::Stream>,
    pub errors: HashMap<String, String>,
    attempts: u8,
    retry_at: Option<Instant>,
    stream_finished: bool,
    providers: Vec<Arc<dyn MusicSource>>,
    covers: HashMap<String, Job<Arc<crate::media::Artwork>>>,
    cover_attempts: std::collections::HashSet<String>,
}
impl State {
    pub fn cancel_playback(&mut self) {
        self.preparing = None;
        self.stream = None;
        self.retry_at = None;
        self.stream_finished = false;
    }
    pub fn reset_attempts(&mut self) {
        self.attempts = 0;
    }
    pub fn busy(&self) -> bool {
        self.preparing.is_some() || self.retry_at.is_some()
    }

    pub(super) fn clear_search(&mut self) {
        self.search = None;
        self.songs.clear();
        self.page = 0;
        self.next = false;
    }
    pub fn new(_settings: &music_sources::Settings) -> Self {
        Self {
            open: false,
            keyword: String::new(),
            searched: String::new(),
            page: 0,
            next: false,
            songs: Vec::new(),
            search: None,
            download: None,
            downloading: None,
            status: String::new(),
            failed: false,
            preparing: None,
            stream: None,
            errors: HashMap::new(),
            attempts: 0,
            retry_at: None,
            stream_finished: false,
            providers: music_sources::sources("").unwrap_or_default(),
            covers: HashMap::new(),
            cover_attempts: Default::default(),
        }
    }
}
impl App {
    pub(super) fn request_online_art(&mut self, record: &library::Record, ctx: &egui::Context) {
        if self.online_music.covers.len() >= 2
            || self.online_music.cover_attempts.contains(&record.path)
        {
            return;
        }
        let Some(song) = &record.online else { return };
        let Some(url) = song
            .cover
            .as_ref()
            .and_then(|s| s.parse::<reqwest::Url>().ok())
        else {
            return;
        };
        if !matches!(url.scheme(), "https" | "http")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return;
        }
        let directory = Settings::queue_path().with_file_name("music-covers");
        let key = url.to_string();
        let ctx = ctx.clone();
        self.online_music.cover_attempts.insert(record.path.clone());
        self.online_music.covers.insert(
            record.path.clone(),
            Job::spawn(move |_| async move {
                let result = crate::comics::cache::fetch(&directory, &key, url, 240)
                    .await
                    .and_then(|image| {
                        let tile = image
                            .bitmap
                            .tiles
                            .into_iter()
                            .next()
                            .ok_or_else(|| anyhow::anyhow!("Invalid music cover"))?;
                        Ok(Arc::new(crate::media::Artwork {
                            width: tile.width() as usize,
                            height: tile.height() as usize,
                            rgba: tile.bytes().to_vec(),
                        }))
                    });
                ctx.request_repaint();
                result
            }),
        );
    }
    pub(super) fn start_online_record(
        &mut self,
        online: music_sources::OnlineSong,
        resume: Option<f64>,
    ) {
        if self.online_music.downloading.as_deref() == Some(&online.key()) {
            self.online_music.download = None;
            self.online_music.downloading = None;
        }
        self.mode = UiMode::Music;
        self.ui_kind_music = Some(true);
        self.workspace.page = workspace::Page::Listening;
        self.settings_open = false;
        self.player = None;
        self.source = None;
        self.media_info = None;
        self.artwork = None;
        self.subtitle_job = None;
        self.subtitles.clear();
        self.lyrics = Default::default();
        self.video.clear();
        self.loading = true;
        self.title = online.title.clone();
        self.online_origin = Some(online.clone());
        if !self
            .library
            .records
            .iter()
            .any(|r| r.online.as_ref().is_some_and(|s| s.key() == online.key()))
        {
            self.library
                .bind_online(format!("online://{}", online.key()), online.clone());
        }
        self.error = None;
        self.workspace.resume = resume.map(|p| (format!("online://{}", online.key()), p));
        if let Some(path) = self
            .library
            .records
            .iter()
            .find(|r| {
                r.online.as_ref().is_some_and(|s| s.key() == online.key())
                    && std::path::Path::new(&r.path).is_file()
            })
            .map(|r| r.path.clone())
        {
            self.load_online_media(path, online);
            return;
        }
        let result = online.song().and_then(|song| {
            let source = music_sources::sources(&self.settings.music.key()?)?
                .into_iter()
                .find(|p| p.descriptor().id == song.source)
                .ok_or_else(|| anyhow::anyhow!("音乐源暂不可用 / Music source is unavailable"))?;
            Ok((song, source))
        });
        match result {
            Ok((song, source)) => {
                let quality = self.settings.music.quality.clone();
                let directory = self.settings.music.directory();
                self.online_music.preparing = Some(Job::spawn(move |_| async move {
                    music_sources::streaming::prepare(source, song, quality, directory).await
                }));
            }
            Err(error) => self.fail_online_playback(error),
        }
    }
    fn load_online_media(&mut self, path: String, online: music_sources::OnlineSong) {
        let progressive = path.starts_with("http://");
        self.load_media(path, progressive);
        let binding = self
            .library
            .records
            .iter()
            .find(|r| r.online.as_ref().is_some_and(|s| s.key() == online.key()))
            .map(|r| r.path.clone())
            .unwrap_or_else(|| format!("online://{}", online.key()));
        self.library.bind_online(binding, online.clone());
        self.title = online.title.clone();
        self.online_origin = Some(online);
        self.ui_kind_music = Some(true);
    }
    pub(super) fn fail_online_playback(&mut self, error: anyhow::Error) {
        let Some(song) = self.online_origin.clone() else {
            return;
        };
        if let Some(player) = &self.player {
            let position = player.position();
            if position > 1.0 {
                self.workspace.resume = Some((format!("online://{}", song.key()), position));
            }
        }
        self.update_library();
        self.player = None;
        self.source = None;
        self.online_music.cancel_playback();
        if self.online_music.attempts == 0 {
            self.online_music.attempts = 1;
            self.online_music.retry_at = Some(Instant::now() + Duration::from_millis(500));
            self.loading = true;
            return;
        }
        let message = music_error(self.settings.language, &error);
        self.online_music.errors.insert(song.key(), message.clone());
        self.error = Some(message);
        self.loading = false;
        let next = self.next_playable_queue_index(true);
        if let Some(next) = next {
            self.play_queue(next);
        }
    }
    fn downloaded_music(&mut self, path: PathBuf) {
        if let Some(song) = crate::media::LocalMetadata::load(&path).online {
            let active = self
                .online_origin
                .as_ref()
                .is_some_and(|s| s.key() == song.key());
            self.library
                .bind_online(path.to_string_lossy().into_owned(), song);
            if active {
                self.lyrics = crate::lyrics::Lyrics::load_for(&path).unwrap_or_default();
            }
        }
        self.metadata_cache.remove(path.to_string_lossy().as_ref());
        self.art_textures.remove(path.to_string_lossy().as_ref());
    }
    pub(super) fn poll_online_music(&mut self, ctx: &egui::Context) {
        let ready: Vec<_> = self
            .online_music
            .covers
            .iter()
            .filter_map(|(path, job)| job.poll().map(|r| (path.clone(), r)))
            .collect();
        for (path, result) in ready {
            self.online_music.covers.remove(&path);
            if let Ok(artwork) = result {
                if self.active_record_path().as_deref() == Some(&path) {
                    self.artwork = Some(ctx.load_texture(
                        "online-music-artwork",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [artwork.width, artwork.height],
                            &artwork.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    ));
                }
                self.metadata_cache.complete(
                    path,
                    Some(Arc::new(crate::media::Info {
                        kind: crate::media::Kind::Music,
                        title: None,
                        artist: None,
                        album: None,
                        artwork: Some(artwork),
                    })),
                );
            }
        }
        if self
            .online_music
            .retry_at
            .is_some_and(|at| Instant::now() >= at)
        {
            self.online_music.retry_at = None;
            if let Some(song) = self.online_origin.clone() {
                let resume = self.workspace.resume.as_ref().map(|(_, p)| *p);
                self.start_online_record(song, resume);
            }
        }
        if let Some(result) = self.online_music.preparing.as_ref().and_then(Job::poll) {
            self.online_music.preparing = None;
            match result {
                Ok(music_sources::streaming::Prepared::Local(path)) => {
                    self.downloaded_music(path.clone());
                    if let Some(song) = self.online_origin.clone() {
                        self.load_online_media(path.to_string_lossy().into_owned(), song);
                    }
                }
                Ok(music_sources::streaming::Prepared::Stream(stream)) => {
                    self.load_online_media(stream.url.clone(), stream.song.clone());
                    self.online_music.stream = Some(stream);
                }
                Err(error) => self.fail_online_playback(error),
            }
        }
        if !self.online_music.stream_finished
            && let Some(result) = self.online_music.stream.as_ref().and_then(|s| s.poll())
        {
            self.online_music.stream_finished = true;
            match result {
                Ok(path) => self.downloaded_music(path),
                Err(error) => self.fail_online_playback(error),
            }
        }
        if self.artwork.is_none()
            && let Some(path) = self.active_record_path()
            && let Some(record) = self
                .library
                .records
                .iter()
                .find(|r| r.path == path)
                .cloned()
        {
            self.request_online_art(&record, ctx);
        }
        if let Some(result) = self.online_music.search.as_ref().and_then(Job::poll) {
            self.online_music.search = None;
            match result {
                Ok(page) => {
                    self.online_music.songs = page.songs;
                    self.online_music.next = page.has_more;
                }
                Err(error) => {
                    self.online_music.failed = true;
                    self.online_music.status = music_error(self.settings.language, &error);
                }
            }
        }
        if let Some(result) = self.online_music.download.as_ref().and_then(Job::poll) {
            self.online_music.download = None;
            self.online_music.downloading = None;
            match result {
                Ok(path) => {
                    self.online_music.failed = false;
                    self.online_music.status = format!(
                        "{} {}",
                        self.settings.language.text("已下载：", "Downloaded:"),
                        path.display()
                    );
                    self.downloaded_music(path);
                }
                Err(error) => {
                    self.online_music.failed = true;
                    self.online_music.status = music_error(self.settings.language, &error);
                }
            }
        }
    }
    pub(super) fn music_buffer_status(&self) -> String {
        let l = self.settings.language;
        if self.online_music.busy() {
            return l.text("正在解析…", "Resolving…").into();
        }
        let buffering = self
            .player
            .as_ref()
            .is_some_and(|p| p.snapshot().state == PlaybackState::Buffering);
        if let Some(stream) = &self.online_music.stream
            && !self.online_music.stream_finished
        {
            let bytes = stream.bytes.load(Ordering::Relaxed);
            let prefix = if buffering {
                l.text("缓冲中", "Buffering")
            } else {
                l.text("下载中", "Downloading")
            };
            return match stream.length() {
                Some(total) => format!("{prefix} {:.0}%", bytes as f64 / total as f64 * 100.0),
                None => format!("{prefix} {:.1} MiB", bytes as f64 / 1048576.0),
            };
        }
        if buffering {
            l.text("缓冲中…", "Buffering…").into()
        } else {
            String::new()
        }
    }
    pub(super) fn preferences_music_sources(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        providers::secret_field(
            ui,
            "music-api-key",
            &mut self.preferences_draft.music.api_key,
            l,
        );
        preferences::form_row(ui, "", l, |ui| {
            ui.hyperlink_to(
                l.text("获取密钥", "Get API key"),
                "https://api.chksz.com/index.html#apis",
            );
        });
        preferences::form_row(ui, l.text("下载目录", "Download directory"), l, |ui| {
            design::input_field(
                ui,
                &mut self.preferences_draft.music.directory,
                ui.available_width(),
                l.text("留空使用应用音乐目录", "Empty: use the app music directory"),
            );
        });
    }
    fn start_music_search(&mut self, source: Arc<dyn MusicSource>, page: usize) {
        self.online_music.page = page;
        self.online_music.songs.clear();
        self.online_music.next = false;
        self.online_music.failed = false;
        self.online_music.status.clear();
        let keyword = self.online_music.searched.clone();
        self.online_music.search = Some(Job::spawn(move |_| async move {
            source.search(&keyword, page).await
        }));
    }
    pub(super) fn online_music_contents(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        let providers = self.online_music.providers.clone();
        if providers.is_empty() {
            return;
        }
        let size = ui.available_size();
        let previous_source = self.settings.music.source.clone();
        let mut selected = providers
            .iter()
            .position(|p| p.descriptor().id == previous_source)
            .unwrap_or(0);
        if !providers
            .iter()
            .any(|p| p.descriptor().id == previous_source)
        {
            self.settings.music.source = providers[0].descriptor().id.into();
        }
        let old_quality = self.settings.music.quality.clone();
        let mut search_page = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            design::dropdown_ui(
                ui,
                "music_provider",
                136.0,
                providers[selected].descriptor().name,
                |ui| {
                    for provider in &providers {
                        let d = provider.descriptor();
                        design::menu_value(
                            ui,
                            &mut self.settings.music.source,
                            d.id.into(),
                            d.name,
                        );
                    }
                },
            );
            selected = providers
                .iter()
                .position(|p| p.descriptor().id == self.settings.music.source)
                .unwrap_or(0);
            let descriptor = providers[selected].descriptor();
            if !descriptor
                .qualities
                .iter()
                .any(|q| q.id == self.settings.music.quality)
            {
                self.settings.music.quality = descriptor.default_quality.into();
            }
            let quality_name = descriptor
                .qualities
                .iter()
                .find(|q| q.id == self.settings.music.quality)
                .map(|q| q.name)
                .unwrap_or(descriptor.default_quality);
            design::dropdown_ui(ui, "music_quality", 136.0, quality_name, |ui| {
                for q in descriptor.qualities {
                    design::menu_value(ui, &mut self.settings.music.quality, q.id.into(), q.name);
                }
            });
            let input_width = (ui.available_width() - design::CONTROL_HEIGHT - 8.0).max(120.0);
            let input = design::search_field(
                ui,
                &mut self.online_music.keyword,
                input_width,
                l.text("歌曲 / 歌手", "Song / artist"),
            );
            if (design::primary_icon_button(ui, icons::MAGNIFYING_GLASS, l.text("搜索", "Search"))
                .clicked()
                || (input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))))
                && !self.online_music.keyword.trim().is_empty()
            {
                self.online_music.searched = self.online_music.keyword.trim().into();
                search_page = Some(0);
            }
        });
        let changed = previous_source != self.settings.music.source;
        if changed {
            self.online_music.clear_search();
        }
        if changed || old_quality != self.settings.music.quality {
            self.preferences_draft.music.source = self.settings.music.source.clone();
            self.preferences_draft.music.quality = self.settings.music.quality.clone();
            if let Err(error) = self.settings.save() {
                self.online_music.status = error.to_string();
            }
        }
        let mut download_song = None;
        ui.add_space(6.0);
        if self.online_music.search.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(l.text("正在搜索…", "Searching…"));
                if design::compact_icon_button(ui, icons::X, l.text("取消", "Cancel"), false)
                    .clicked()
                {
                    self.online_music.search = None;
                }
            });
        } else if self.online_music.songs.is_empty() {
            design::empty_state(
                ui,
                icons::MUSIC_NOTES,
                if self.online_music.searched.is_empty() {
                    l.text("输入关键词搜索在线音乐", "Search online music")
                } else {
                    l.text("未找到匹配歌曲", "No matching songs")
                },
            );
        } else {
            design::scroll()
                .id_salt("music_results")
                .max_height((size.y - 220.0).clamp(140.0, 500.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for (idx, song) in self.online_music.songs.iter().enumerate() {
                        let (row, row_resp) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::click());
                        let row_resp = row_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if row_resp.hovered() {
                            ui.painter().rect_filled(row, 8, SURFACE);
                        }
                        let mut info = workspace::child(
                            ui,
                            ("online-song", idx),
                            Rect::from_min_max(
                                row.min + vec2(12.0, 8.0),
                                pos2(row.max.x - 86.0, row.max.y - 6.0),
                            ),
                        );
                        info.style_mut().interaction.selectable_labels = false;
                        info.spacing_mut().item_spacing.y = 2.0;
                        info.add(egui::Label::new(RichText::new(&song.title).strong()).truncate());
                        info.add(
                            egui::Label::new(
                                RichText::new(format!("{} · {}", song.artist, song.album))
                                    .small()
                                    .color(MUTED),
                            )
                            .truncate(),
                        );
                        let mut actions_ui = workspace::child(
                            ui,
                            ("online-song-actions", idx),
                            Rect::from_min_max(
                                pos2(row.max.x - 80.0, row.min.y + 12.0),
                                pos2(row.max.x - 10.0, row.max.y - 12.0),
                            ),
                        );
                        actions_ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if design::compact_icon_button(
                                ui,
                                icons::PLAY,
                                l.text("播放", "Play"),
                                false,
                            )
                            .clicked()
                                || row_resp.double_clicked()
                            {
                                download_song = Some((song.clone(), true));
                            }
                            let active = self.online_origin.as_ref().is_some_and(|s| {
                                s.key() == music_sources::OnlineSong::from(song).key()
                            }) && (self.online_music.busy()
                                || self.online_music.stream.is_some());
                            ui.add_enabled_ui(
                                self.online_music.download.is_none() && !active,
                                |ui| {
                                    if design::compact_icon_button(
                                        ui,
                                        icons::DOWNLOAD_SIMPLE,
                                        l.text("下载", "Download"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        download_song = Some((song.clone(), false));
                                    }
                                },
                            );
                        });
                    }
                });
            if self.online_music.page > 0 || self.online_music.next {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.add_enabled_ui(
                        self.online_music.page > 0 && self.online_music.search.is_none(),
                        |ui| {
                            if design::icon_button(
                                ui,
                                icons::CARET_LEFT,
                                l.text("上一页", "Previous page"),
                                false,
                            )
                            .clicked()
                            {
                                search_page = Some(self.online_music.page - 1);
                            }
                        },
                    );
                    ui.label(
                        RichText::new(format!("{}", self.online_music.page + 1))
                            .strong()
                            .color(MUTED),
                    );
                    ui.add_enabled_ui(
                        self.online_music.next && self.online_music.search.is_none(),
                        |ui| {
                            if design::icon_button(
                                ui,
                                icons::CARET_RIGHT,
                                l.text("下一页", "Next page"),
                                false,
                            )
                            .clicked()
                            {
                                search_page = Some(self.online_music.page + 1);
                            }
                        },
                    );
                });
            }
        }
        if search_page.is_some() || download_song.is_some() {
            let result = self
                .settings
                .music
                .key()
                .and_then(|key| music_sources::sources(&key));
            match result {
                Ok(providers) => {
                    let source = providers[selected].clone();
                    if let Some(page) = search_page {
                        self.start_music_search(source.clone(), page);
                    }
                    if let Some((song, play)) = download_song {
                        if play {
                            let online = music_sources::OnlineSong::from(&song);
                            let path = self
                                .library
                                .records
                                .iter()
                                .find(|r| {
                                    r.online.as_ref().is_some_and(|s| s.key() == online.key())
                                })
                                .map(|r| r.path.clone())
                                .unwrap_or_else(|| format!("online://{}", online.key()));
                            self.library.bind_online(path.clone(), online.clone());
                            self.play_record(
                                library::Record {
                                    path,
                                    title: online.title.clone(),
                                    music: true,
                                    online: Some(online),
                                    ..Default::default()
                                },
                                false,
                            );
                        } else {
                            let quality = self.settings.music.quality.clone();
                            let directory = self.settings.music.directory();
                            self.online_music.failed = false;
                            self.online_music.status.clear();
                            self.online_music.downloading =
                                Some(music_sources::OnlineSong::from(&song).key());
                            self.online_music.download =
                                Some(Job::spawn(move |bytes| async move {
                                    music_sources::download(
                                        source.as_ref(),
                                        &song,
                                        &quality,
                                        &directory,
                                        bytes,
                                    )
                                    .await
                                }));
                        }
                    }
                }
                Err(error) => {
                    self.online_music.failed = true;
                    self.online_music.status = music_error(l, &error);
                }
            }
        }
        if let Some(job) = &self.online_music.download {
            let bytes = job.bytes.load(Ordering::Relaxed);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "{} {:.1} MiB",
                    l.text("解析 / 下载中", "Resolving / downloading"),
                    bytes as f64 / 1048576.0
                ));
                if design::compact_icon_button(
                    ui,
                    icons::X,
                    l.text("取消下载", "Cancel download"),
                    false,
                )
                .clicked()
                {
                    self.online_music.download = None;
                    self.online_music.downloading = None;
                    self.online_music.status = l.text("已取消", "Cancelled").into();
                }
            });
        }
        if !self.online_music.status.is_empty() {
            if self.online_music.failed {
                ui.colored_label(Color32::LIGHT_RED, &self.online_music.status);
            } else {
                ui.small(RichText::new(&self.online_music.status).color(MUTED));
            }
        }
    }
    pub(super) fn online_music_window(&mut self, ctx: &egui::Context) {
        if !self.online_music.open {
            return;
        }
        let l = self.settings.language;
        let mut open = true;
        // Constructing providers does not perform network requests or load a key.
        let providers = self.online_music.providers.clone();
        if providers.is_empty() {
            return;
        }
        let size = ctx.content_rect().size();
        egui::Window::new(l.text("搜索音乐", "Search music"))
            .id(Id::new("online_music"))
            .open(&mut open)
            .collapsible(false)
            .default_width((size.x - 80.0).clamp(320.0, 760.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                self.online_music_contents(ui);
            });
        self.online_music.open &= open;
    }
}
