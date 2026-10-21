use super::*;
use crate::music_sources::{self, Job, MusicSource, SearchPage, Song};
use std::sync::atomic::Ordering;

fn music_error(language: Language, error: &anyhow::Error) -> String {
    let message = error.to_string();
    if language == Language::English {
        return message;
    }
    let translated = match message.as_str() {
        "Set CHKSZ_API_KEY in .env or Music sources settings" => {
            "请在 .env 中配置 CHKSZ_API_KEY，或在音乐源设置中填写密钥"
        }
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
        "Cannot read .env" | "Cannot parse .env" => "无法读取或解析 .env 配置文件",
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
    pub draft: music_sources::Settings,
    keyword: String,
    searched: String,
    page: usize,
    next: bool,
    songs: Vec<Song>,
    search: Option<Job<SearchPage>>,
    download: Option<Job<PathBuf>>,
    status: String,
    failed: bool,
    pub autoplay: bool,
    providers: Vec<Arc<dyn MusicSource>>,
}
impl State {
    pub(super) fn clear_search(&mut self) {
        self.search = None;
        self.songs.clear();
        self.page = 0;
        self.next = false;
    }
    pub fn new(settings: &music_sources::Settings) -> Self {
        Self {
            open: false,
            draft: settings.clone(),
            keyword: String::new(),
            searched: String::new(),
            page: 0,
            next: false,
            songs: Vec::new(),
            search: None,
            download: None,
            status: String::new(),
            failed: false,
            autoplay: false,
            providers: music_sources::sources("").unwrap_or_default(),
        }
    }
}
impl App {
    pub(super) fn poll_online_music(&mut self) {
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
            match result {
                Ok(path) => {
                    self.metadata_cache.remove(path.to_string_lossy().as_ref());
                    self.art_textures.remove(path.to_string_lossy().as_ref());
                    self.online_music.failed = false;
                    self.online_music.status = format!(
                        "{} {}",
                        self.settings.language.text("已下载：", "Downloaded:"),
                        path.display()
                    );
                    if self.online_music.autoplay {
                        self.online_music.autoplay = false;
                        self.set_active_queue(true);
                        let text = path.to_string_lossy().into_owned();
                        let index = self.queue.items.iter().position(|item| matches!(&item.source, crate::queue::Source::File(p) if p == &text)).unwrap_or_else(|| {
                            let index = self.queue.items.len();
                            self.queue.items.push(crate::queue::Item::file(text)); index
                        });
                        self.mode = UiMode::Music;
                        self.play_queue(index);
                    }
                }
                Err(error) => {
                    self.online_music.autoplay = false;
                    self.online_music.failed = true;
                    self.online_music.status = music_error(self.settings.language, &error);
                }
            }
        }
    }
    pub(super) fn preferences_music_sources(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        ui.heading(l.text("在线音乐", "Online music"));
        ui.label(l.text(
            "ChKSz · 网易云 / QQ 音乐 / 酷狗",
            "ChKSz · NetEase / QQ Music / Kugou",
        ));
        ui.hyperlink_to(
            l.text("接口文档与 API Key", "API documentation and key"),
            "https://api.chksz.com/index.html#apis",
        );
        ui.add_space(12.0);
        ui.label("ChKSz API Key");
        ui.add(
            egui::TextEdit::singleline(&mut self.online_music.draft.api_key)
                .password(true)
                .hint_text(l.text("留空使用 CHKSZ_API_KEY", "Empty: use CHKSZ_API_KEY")),
        );
        ui.label(
            RichText::new(l.text(
                "密钥仅本次运行有效；永久配置请使用 .env",
                "The key is session-only; use .env for persistent configuration",
            ))
            .small()
            .color(MUTED),
        );
        ui.add_space(12.0);
        ui.label(l.text("下载目录", "Download directory"));
        ui.add(
            egui::TextEdit::singleline(&mut self.online_music.draft.directory)
                .desired_width(f32::INFINITY)
                .hint_text(l.text("留空使用应用音乐目录", "Empty: use the app music directory")),
        );
        ui.label(
            RichText::new(self.online_music.draft.directory().display().to_string())
                .small()
                .color(MUTED),
        );
        ui.add_space(12.0);
        ui.label(l.text(
            "播放会先下载到本地，再加入音乐队列。下载文件会保留，不自动删除。",
            "Play downloads a local file and adds it to the music queue. Downloads are retained.",
        ));
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
        let selected = providers
            .iter()
            .position(|p| p.descriptor().id == previous_source)
            .unwrap_or(0);
        if !providers
            .iter()
            .any(|p| p.descriptor().id == previous_source)
        {
            self.settings.music.source = providers[0].descriptor().id.into();
        }
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("music_provider")
                .selected_text(providers[selected].descriptor().name)
                .show_ui(ui, |ui| {
                    for provider in &providers {
                        let d = provider.descriptor();
                        ui.selectable_value(&mut self.settings.music.source, d.id.into(), d.name);
                    }
                });
            if ui.button(l.text("音乐源设置", "Source settings")).clicked() {
                self.online_music.open = false;
                self.settings_open = true;
                self.preferences_tab = PrefTab::MusicSources;
                self.config_message.clear();
            }
        });
        let selected = providers
            .iter()
            .position(|p| p.descriptor().id == self.settings.music.source)
            .unwrap_or(0);
        let descriptor = providers[selected].descriptor();
        let changed = previous_source != self.settings.music.source;
        if changed {
            self.online_music.clear_search();
        }
        if !descriptor
            .qualities
            .iter()
            .any(|q| q.id == self.settings.music.quality)
        {
            self.settings.music.quality = descriptor.default_quality.into();
        }
        let old_quality = self.settings.music.quality.clone();
        ui.horizontal(|ui| {
            ui.label(l.text("音质", "Quality"));
            egui::ComboBox::from_id_salt("music_quality")
                .selected_text(
                    descriptor
                        .qualities
                        .iter()
                        .find(|q| q.id == self.settings.music.quality)
                        .unwrap()
                        .name,
                )
                .show_ui(ui, |ui| {
                    for q in descriptor.qualities {
                        ui.selectable_value(&mut self.settings.music.quality, q.id.into(), q.name);
                    }
                });
        });
        if changed || old_quality != self.settings.music.quality {
            self.online_music.draft.source = self.settings.music.source.clone();
            self.online_music.draft.quality = self.settings.music.quality.clone();
            if let Err(error) = self.settings.save() {
                self.online_music.status = error.to_string();
            }
        }
        let mut search_page = None;
        ui.horizontal(|ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.online_music.keyword)
                    .hint_text(l.text("歌曲 / 歌手", "Song / artist"))
                    .desired_width((ui.available_width() - 90.0).max(100.0)),
            );
            if (ui.button(l.text("搜索", "Search")).clicked()
                || (input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))))
                && !self.online_music.keyword.trim().is_empty()
            {
                self.online_music.searched = self.online_music.keyword.trim().into();
                search_page = Some(0);
            }
        });
        let mut download_song = None;
        ui.separator();
        if self.online_music.search.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(l.text("正在搜索…", "Searching…"));
                if ui.button(l.text("取消", "Cancel")).clicked() {
                    self.online_music.search = None;
                }
            });
        } else if self.online_music.songs.is_empty() {
            ui.label(l.text(
                "输入关键词搜索；暂无搜索结果",
                "Search by keyword; no results yet",
            ));
        }
        egui::ScrollArea::vertical()
            .id_salt("music_results")
            .max_height((size.y - 340.0).clamp(100.0, 420.0))
            .show(ui, |ui| {
                for song in &self.online_music.songs {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 150.0).max(130.0));
                            ui.label(RichText::new(&song.title).strong());
                            ui.label(
                                RichText::new(format!("{} · {}", song.artist, song.album))
                                    .small()
                                    .color(MUTED),
                            );
                        });
                        ui.add_enabled_ui(self.online_music.download.is_none(), |ui| {
                            if ui.button(l.text("播放", "Play")).clicked() {
                                download_song = Some((song.clone(), true));
                            }
                            if ui.button(l.text("下载", "Download")).clicked() {
                                download_song = Some((song.clone(), false));
                            }
                        });
                    });
                    ui.separator();
                }
            });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.online_music.page > 0 && self.online_music.search.is_none(),
                    egui::Button::new(l.text("上一页", "Previous")),
                )
                .clicked()
            {
                search_page = Some(self.online_music.page - 1);
            }
            ui.label(format!("{}", self.online_music.page + 1));
            if ui
                .add_enabled(
                    self.online_music.next && self.online_music.search.is_none(),
                    egui::Button::new(l.text("下一页", "Next")),
                )
                .clicked()
            {
                search_page = Some(self.online_music.page + 1);
            }
        });
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
                        let quality = self.settings.music.quality.clone();
                        let directory = self.settings.music.directory();
                        self.online_music.autoplay = play;
                        self.online_music.failed = false;
                        self.online_music.status.clear();
                        self.online_music.download = Some(Job::spawn(move |bytes| async move {
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
                if ui.button(l.text("取消下载", "Cancel download")).clicked() {
                    self.online_music.download = None;
                    self.online_music.autoplay = false;
                    self.online_music.status = l.text("已取消", "Cancelled").into();
                }
            });
        }
        if !self.online_music.status.is_empty() {
            if self.online_music.failed {
                ui.colored_label(
                    Color32::LIGHT_RED,
                    l.text(
                        "操作失败，请检查密钥、网络或尝试其他音质。",
                        "Failed: check your key, network or try another quality.",
                    ),
                );
            }
            ui.label(&self.online_music.status);
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
