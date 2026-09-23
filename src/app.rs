use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Id, Key, Layout, Painter, Rect, Response, RichText, Sense,
    Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::player::{Event, PlaybackState, Player};
use crate::renderer::VideoRenderer;
use crate::settings::{Language, Settings};
use crate::subtitles::{self, Cue, Job};
mod about;
mod design;
mod magnets;
mod music;
mod preferences;
mod qq;
use design::{ACCENT, BACKGROUND, MUTED, SURFACE};

const BAR_BOTTOM: f32 = 68.0;
const BAR_TOP: f32 = 60.0;
const TOOLBAR_HIDE_DELAY: Duration = Duration::from_millis(400);
type ArtworkResult = (String, Option<Arc<crate::media::Artwork>>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Music,
    Video,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefTab {
    General,
    Subtitles,
    About,
}
enum Act {
    None,
    TogglePlay,
    Seek(f64),
    Volume(f32),
    ToggleMute,
    ToggleFullscreen,
    Open,
    AddFiles,
    Previous,
    Next,
    QueuePlay(usize),
    QueueRemove(usize),
    GenerateSubtitles,
    CancelSubtitles,
    ExportSubtitles,
}

pub struct App {
    queue: crate::queue::Queue,
    queue_inactive: crate::queue::Queue,
    queue_kind_music: bool,
    queue_open: bool,
    queue_drag: Option<usize>,
    art_requests: Sender<String>,
    art_results: Receiver<ArtworkResult>,
    art_cache: HashMap<String, Option<Arc<crate::media::Artwork>>>,
    art_textures: HashMap<String, egui::TextureHandle>,
    media_info: Option<Arc<crate::media::Info>>,
    artwork: Option<egui::TextureHandle>,
    append_dialog: bool,
    settings: Settings,
    settings_open: bool,
    preferences_tab: PrefTab,
    preferences_armed: bool,
    preferences_rect: Option<Rect>,
    mode: UiMode,
    updater: crate::updater::Updater,
    torrent_job: Option<crate::torrent::Job>,
    torrent_files: Vec<crate::torrent::File>,
    torrent_selected: Option<crate::torrent::File>,
    torrent_progress: crate::torrent::Progress,
    torrent_status: String,
    torrent_resolving: bool,
    magnet_open: bool,
    magnet_input: String,
    llm_draft: crate::settings::LlmSettings,
    llm_defaults: crate::settings::LlmSettings,
    env_key_available: bool,
    config_message: String,
    player: Option<Player>,
    video: VideoRenderer,
    title: String,
    source: Option<PathBuf>,
    subtitle_job: Option<Job>,
    subtitles: Vec<Cue>,
    subtitles_visible: bool,
    subtitle_status: String,
    subtitle_skipped: Vec<(f64, f64, String)>,
    subtitle_save_tx: Sender<Result<PathBuf, String>>,
    subtitle_save_rx: Receiver<Result<PathBuf, String>>,
    subtitle_metrics: String,
    open_tx: Sender<Option<Vec<PathBuf>>>,
    open_rx: Receiver<Option<Vec<PathBuf>>>,
    dialog_open: bool,
    qq_key_rx: Option<Receiver<qq::KeyResult>>,
    scrubbing: bool,
    scrub: f64,
    pending_seek: Option<(u64, f64)>,
    vol: f32,
    muted: bool,
    volume_open: bool,
    loop_single: bool,
    fullscreen: bool,
    ui_alpha: f32,
    last_toolbar_hover: Option<Instant>,
    error: Option<String>,
    frames: u64,
    last_stats: Instant,
    stats: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Vec<String>) -> Self {
        setup_fonts(&cc.egui_ctx);
        design::apply(&cc.egui_ctx);
        let settings = Settings::load();
        let mut defaults = subtitles::LlmConfig::defaults().unwrap_or_default();
        let env_key_available = !defaults.api_key.is_empty();
        defaults.api_key.clear();
        let (open_tx, open_rx) = unbounded();
        let (subtitle_save_tx, subtitle_save_rx) = unbounded();
        let (art_requests, art_requests_rx): (Sender<String>, Receiver<String>) = unbounded();
        let (art_results_tx, art_results): (Sender<ArtworkResult>, Receiver<ArtworkResult>) =
            unbounded();
        thread::spawn(move || {
            while let Ok(path) = art_requests_rx.recv() {
                let art = crate::media::probe_artwork(&path);
                if art_results_tx.send((path, art)).is_err() {
                    break;
                }
            }
        });
        let mut app = Self {
            queue: Default::default(),
            queue_inactive: Default::default(),
            queue_kind_music: true,
            queue_open: true,
            queue_drag: None,
            art_requests,
            art_results,
            art_cache: HashMap::new(),
            art_textures: HashMap::new(),
            media_info: None,
            artwork: None,
            append_dialog: false,
            updater: crate::updater::Updater::new(settings.auto_check_updates),
            torrent_job: None,
            torrent_files: Vec::new(),
            torrent_selected: None,
            torrent_progress: Default::default(),
            torrent_status: String::new(),
            torrent_resolving: false,
            magnet_open: false,
            magnet_input: String::new(),
            preferences_tab: PrefTab::General,
            preferences_armed: false,
            preferences_rect: None,
            mode: UiMode::Music,
            llm_draft: settings.llm.clone(),
            settings,
            settings_open: false,
            llm_defaults: defaults,
            env_key_available,
            config_message: String::new(),
            player: None,
            video: VideoRenderer::new(cc.wgpu_render_state.clone()),
            title: String::new(),
            source: None,
            subtitle_job: None,
            subtitles: Vec::new(),
            subtitles_visible: true,
            subtitle_status: String::new(),
            subtitle_skipped: Vec::new(),
            subtitle_metrics: String::new(),
            subtitle_save_tx,
            subtitle_save_rx,
            open_tx,
            open_rx,
            dialog_open: false,
            qq_key_rx: None,
            scrubbing: false,
            scrub: 0.0,
            pending_seek: None,
            vol: 1.0,
            muted: false,
            volume_open: false,
            loop_single: false,
            fullscreen: false,
            ui_alpha: 0.0,
            last_toolbar_hover: None,
            error: None,
            frames: 0,
            last_stats: Instant::now(),
            stats: std::env::var_os("REPLAYER_STATS").is_some(),
        };
        if let Ok(bytes) = std::fs::read(Settings::queue_path())
            && let Ok(saved) = serde_json::from_slice::<crate::queue::SavedQueues>(&bytes)
        {
            app.queue.restore(saved.music);
            app.queue_inactive.restore(saved.video);
        }
        if !initial.is_empty() {
            app.open_items(initial, false);
        }
        app
    }

    fn persist_queue(&self) {
        let path = Settings::queue_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let (music, video) = if self.queue_kind_music {
            (self.queue.saved(), self.queue_inactive.saved())
        } else {
            (self.queue_inactive.saved(), self.queue.saved())
        };
        let saved = crate::queue::SavedQueues { music, video };
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&saved).unwrap_or_default());
    }
    /// The music and video queues are stored independently; switching swaps them.
    fn set_active_queue(&mut self, music: bool) {
        if self.queue_kind_music == music {
            return;
        }
        std::mem::swap(&mut self.queue, &mut self.queue_inactive);
        self.queue_kind_music = music;
    }

    fn load(&mut self, path: String) {
        if path.trim().starts_with("magnet:") {
            self.magnet_input = path;
            self.start_magnet();
            return;
        }
        self.torrent_job = None;
        self.remove_torrent_queue();
        self.torrent_selected = None;
        self.torrent_files.clear();
        self.torrent_resolving = false;
        self.magnet_open = false;
        self.torrent_status.clear();
        self.torrent_progress = Default::default();
        self.load_media(path, false);
    }
    fn load_media(&mut self, path: String, progressive: bool) {
        self.load_media_with_key(path, progressive, None);
    }
    fn load_media_with_key(&mut self, path: String, progressive: bool, key: Option<String>) {
        self.media_info = None;
        self.artwork = None;
        self.subtitle_job = None;
        self.subtitles.clear();
        self.subtitle_status.clear();
        self.subtitle_skipped.clear();
        self.subtitle_metrics.clear();
        self.source = Some(PathBuf::from(&path));
        self.player = None;
        self.video.clear();
        self.scrub = 0.0;
        self.pending_seek = None;
        self.scrubbing = false;
        self.ui_alpha = 0.0;
        self.last_toolbar_hover = None;
        self.error = None;
        self.title = PathBuf::from(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(path.clone());
        match if let Some(key) = key {
            Player::open_with_qq_key(path, key)
        } else if progressive {
            Player::open_progressive(path)
        } else {
            Player::open(path)
        } {
            Ok(p) => {
                p.set_volume(if self.muted { 0.0 } else { self.vol });
                self.player = Some(p);
            }
            Err(e) => self.error = Some(format!("{e}")),
        }
    }

    fn open_dialog(&mut self) {
        if self.dialog_open {
            return;
        }
        self.dialog_open = true;
        let tx = self.open_tx.clone();
        let language = self.settings.language;
        thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title(language.text("选择音频或视频", "Choose audio or video"))
                .add_filter(
                    language.text("媒体文件", "Media files"),
                    &crate::media::AUDIO_EXTENSIONS
                        .iter()
                        .chain(crate::media::VIDEO_EXTENSIONS)
                        .chain(crate::media_source::QQ_EXTENSIONS)
                        .copied()
                        .collect::<Vec<_>>(),
                )
                .pick_files();
            let _ = tx.send(picked);
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let screen = root.max_rect();
        let painter = root.painter().clone();
        let language = self.settings.language;
        let previous_concurrency = self.settings.subtitle_concurrency;
        self.updater.poll();
        self.poll_torrent();
        self.poll_qq_key();
        while let Ok((path, art)) = self.art_results.try_recv() {
            self.art_cache.insert(path, art);
        }
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        if !dropped.is_empty() {
            self.open_items(dropped, ctx.input(|i| i.modifiers.shift));
        }

        if let Ok(picked) = self.open_rx.try_recv() {
            self.dialog_open = false;
            if let Some(paths) = picked {
                self.open_items(
                    paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect(),
                    self.append_dialog,
                );
            }
            self.append_dialog = false;
        }

        let mut advance = false;
        if let Some(player) = &self.player {
            while let Some(event) = player.poll_event() {
                match event {
                    Event::MediaInfo(info) => {
                        if let Some(title) = &info.title {
                            self.title = title.clone();
                            if let Some(i) = self.queue.current
                                && let Some(item) = self.queue.items.get_mut(i)
                            {
                                item.title = title.clone();
                            }
                        }
                        self.artwork = info.artwork.as_ref().map(|cover| {
                            ctx.load_texture(
                                "music-artwork",
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [cover.width, cover.height],
                                    &cover.rgba,
                                ),
                                egui::TextureOptions::LINEAR,
                            )
                        });
                        if info.kind == crate::media::Kind::Music && self.fullscreen {
                            self.fullscreen = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                        }
                        if let Some(source) = &self.source {
                            self.art_cache.insert(
                                source.to_string_lossy().into_owned(),
                                info.artwork.clone(),
                            );
                        }
                        self.media_info = Some(info);
                    }
                    Event::Ended => advance = true,
                    Event::SeekCompleted { id, .. } => {
                        if self.pending_seek.is_some_and(|(pending, _)| pending == id) {
                            self.pending_seek = None;
                        }
                    }
                    Event::Error(error) | Event::Warning(error) => self.error = Some(error),
                    _ => {}
                }
            }
        }
        if advance {
            let replay = |app: &mut App| {
                if let Some(p) = &app.player {
                    p.set_playing(true);
                }
            };
            if self.loop_single {
                replay(self);
            } else {
                let next = self
                    .queue
                    .next_index()
                    .or_else(|| (!self.queue.items.is_empty()).then_some(0));
                match next {
                    Some(next) if Some(next) != self.queue.current => self.play_queue(next),
                    Some(_) | None => replay(self),
                }
            }
        }
        let music = self.is_music();
        let sidebar_width = if self.queue_open && !self.queue.items.is_empty() {
            (screen.width() * 0.38).clamp(220.0, 340.0)
        } else {
            0.0
        };
        let playback_rect =
            Rect::from_min_max(screen.min, pos2(screen.max.x - sidebar_width, screen.max.y));
        let mut subtitle_done = false;
        if let Some(job) = &self.subtitle_job {
            while let Ok(event) = job.events.try_recv() {
                match event {
                    subtitles::Event::WaitingCache => {
                        self.subtitle_status = language
                            .text(
                                "等待已下载音频，播放下载优先",
                                "Waiting for cached audio; playback has priority",
                            )
                            .into()
                    }
                    subtitles::Event::Cues(cues) => {
                        self.subtitles.extend(cues);
                        self.subtitles.sort_by(|a, b| a.start.total_cmp(&b.start));
                    }
                    subtitles::Event::Progress { through, total } => {
                        self.subtitle_status = if total > 0.0 {
                            format!(
                                "{} · {:.0}%",
                                language.text("正在生成字幕", "Generating subtitles"),
                                (through / total * 100.0).min(100.0)
                            )
                        } else {
                            format!(
                                "{} · {}",
                                language.text("正在生成字幕", "Generating subtitles"),
                                fmt_time(through)
                            )
                        };
                    }
                    subtitles::Event::Finished => {
                        self.subtitle_status = if self.subtitles.is_empty() {
                            language.text("未识别到语音", "No speech detected").into()
                        } else {
                            format!(
                                "{}: {}",
                                language.text("已生成字幕", "Subtitles generated"),
                                self.subtitles.len()
                            )
                        };
                        if !self.subtitle_skipped.is_empty() {
                            self.subtitle_status = format!(
                                "{} · {} {}",
                                language.text("生成结束", "Generation finished"),
                                self.subtitle_skipped.len(),
                                language.text("段失败已跳过", "failed segments skipped")
                            );
                        }
                        subtitle_done = true;
                    }
                    subtitles::Event::Skipped { start, end, error } => {
                        self.subtitle_skipped.push((start, end, error));
                    }
                    subtitles::Event::Failed(error) => {
                        self.subtitle_status = language
                            .text(
                                "字幕生成失败，可重试；已生成的部分可以导出",
                                "Generation failed. Retry or export the completed subtitles.",
                            )
                            .into();
                        self.error = Some(error);
                        subtitle_done = true;
                    }
                    subtitles::Event::Metrics {
                        seconds,
                        peak,
                        fast,
                        short,
                    } => {
                        self.subtitle_metrics = format!(
                            "{} {seconds:.1}s · {} {peak}\n{} {fast} · {} {short}",
                            language.text("耗时", "Time"),
                            language.text("峰值并发", "Peak requests"),
                            language.text("阅读偏快", "Fast cues"),
                            language.text("时长偏短", "Short cues")
                        );
                    }
                }
            }
        }
        if subtitle_done {
            self.subtitle_job = None;
        }
        while let Ok(result) = self.subtitle_save_rx.try_recv() {
            match result {
                Ok(path) => {
                    self.subtitle_status = format!(
                        "{}: {}",
                        language.text("字幕已导出", "Subtitles exported"),
                        path.display()
                    )
                }
                Err(error) => self.error = Some(error),
            }
        }
        let due = self.player.as_mut().and_then(|p| {
            let now = p.sync_time();
            p.next_frame(now)
        });
        if let Some(f) = due {
            self.frames += 1;
            if let Err(error) = self.video.upload(&ctx, &f) {
                self.error = Some(format!("render video: {error:#}"));
            }
        }

        // Only the top/bottom edge regions reveal the toolbars. Playback state
        // and activity over the picture do not keep them visible.
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let (playing, dur, pos) = match &self.player {
            Some(p) => (p.is_playing(), p.duration(), p.position()),
            None => (false, 0.0, 0.0),
        };
        if let Some(job) = &self.subtitle_job {
            job.prioritize(self.pending_seek.map_or(pos, |(_, target)| target));
        }
        let in_bar = pointer
            .map(|p| {
                self.player.is_some()
                    && screen.contains(p)
                    && (p.y > screen.max.y - BAR_BOTTOM || p.y < screen.min.y + BAR_TOP)
            })
            .unwrap_or(false);
        let dragging_toolbar = self.ui_alpha > 0.02 && ctx.dragged_id().is_some();
        if in_bar || self.scrubbing || dragging_toolbar || egui::Popup::is_any_open(&ctx) {
            self.last_toolbar_hover = Some(Instant::now());
        }
        let show = music
            || self.player.is_none()
            || self
                .last_toolbar_hover
                .is_some_and(|last| last.elapsed() < TOOLBAR_HIDE_DELAY);
        let target = if show { 1.0 } else { 0.0 };
        let dt = ctx.input(|i| i.unstable_dt).min(0.1);
        self.ui_alpha += (target - self.ui_alpha) * (dt * 9.0).clamp(0.0, 1.0);
        if self.ui_alpha < 0.02 {
            self.ui_alpha = if target > 0.0 { 0.02 } else { 0.0 };
        } else if self.ui_alpha > 0.98 && show {
            self.ui_alpha = 1.0;
        }
        let a = self.ui_alpha;

        // ---- video background ----
        painter.rect_filled(
            screen,
            CornerRadius::same(0),
            if self.player.is_some() && !music {
                Color32::BLACK
            } else {
                BACKGROUND
            },
        );
        let mut video_rect = playback_rect;
        if let Some((texture, ar)) = self.video.current() {
            let mut w = playback_rect.width();
            let mut h = w / ar;
            if h > playback_rect.height() {
                h = playback_rect.height();
                w = h * ar;
            }
            let vr = Rect::from_center_size(playback_rect.center(), vec2(w, h));
            video_rect = vr;
            painter.image(
                texture,
                vr,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // Subtitles remain visible when the toolbars are hidden and follow seeks.
        if !music
            && self.subtitles_visible
            && let Some(text) = subtitles::active(&self.subtitles, pos)
        {
            paint_subtitle(&painter, video_rect, screen, text);
        }

        // ---- video click area ----
        let mut click_rect = playback_rect;
        if self.player.is_some() {
            click_rect.max.y = screen.max.y - BAR_BOTTOM;
            click_rect.min.y = screen.min.y + BAR_TOP;
        }
        let vresp = root.interact(
            click_rect,
            Id::new("video_click"),
            if music {
                Sense::hover()
            } else {
                Sense::click()
            },
        );

        let mut act = Act::None;
        if self.player.is_some() && !music && !self.settings_open && !self.magnet_open {
            if vresp.double_clicked() {
                act = Act::ToggleFullscreen;
            } else if vresp.clicked() {
                act = Act::TogglePlay;
            }
        }

        // ---- keyboard ----
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.settings_open {
                self.settings_open = false;
            } else if self.magnet_open {
                self.magnet_open = false;
            } else {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
        }
        if self.player.is_some()
            && !self.settings_open
            && !self.magnet_open
            && !ctx.egui_wants_keyboard_input()
        {
            if ctx.input(|i| i.key_pressed(Key::Space)) {
                act = Act::TogglePlay;
            }
            if !music && ctx.input(|i| i.key_pressed(Key::F)) {
                act = Act::ToggleFullscreen;
            }
            if ctx.input(|i| i.key_pressed(Key::M)) {
                act = Act::ToggleMute;
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
                act = Act::Seek((pos - 5.0).max(0.0));
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
                act = Act::Seek((pos + 5.0).min(dur));
            }
        }

        // ---- top bar ----
        if a > 0.02 {
            let top_rect = Rect::from_min_max(
                pos2(screen.min.x, screen.min.y),
                pos2(screen.max.x, screen.min.y + BAR_TOP),
            );
            let mut tui = root.new_child(
                UiBuilder::new()
                    .id_salt("topbar")
                    .max_rect(top_rect.shrink2(vec2(20.0, 12.0)))
                    .layout(Layout::top_down(Align::Min)),
            );
            tui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.player.is_some()
                        && icon_button(ui, 26.0, a, draw_folder)
                            .on_hover_text(language.text("打开媒体文件", "Open media files"))
                            .clicked()
                    {
                        act = Act::Open;
                    }
                    let update_ready = matches!(
                        self.updater.status,
                        crate::updater::Status::Available | crate::updater::Status::Ready
                    );
                    let settings_label = if update_ready {
                        language.text("设置 · 有更新", "Settings · Update available")
                    } else {
                        language.text("设置", "Settings")
                    };
                    let settings = icon_button(ui, 26.0, a, |p, r, c| {
                        draw_settings(p, r, c);
                        if update_ready {
                            p.circle_filled(pos2(r.max.x, r.min.y), 3.0, ACCENT);
                        }
                    });
                    if settings.on_hover_text(settings_label).clicked() {
                        self.settings_open = true;
                        self.llm_draft = self.settings.llm.clone();
                        self.config_message.clear();
                    }
                    if self.player.is_some() {
                        if !music && ui.button(language.text("磁链", "Magnet")).clicked(){self.magnet_open=true;}
                        if !music {
                        let caption = if self.subtitle_job.is_some() {
                            language.text("字幕 · 生成中", "Subtitles · generating")
                        } else {
                            language.text("字幕", "Subtitles")
                        };
                        ui.menu_button(
                            RichText::new(caption)
                                .color(Color32::from_white_alpha((235.0 * a) as u8)),
                            |ui| {
                                ui.set_max_width(300.0);
                                if !self.subtitle_status.is_empty() {
                                    ui.label(&self.subtitle_status);
                                    if !self.subtitle_metrics.is_empty() {
                                        ui.small(&self.subtitle_metrics);
                                    }
                                    ui.separator();
                                }
                                if !self.subtitle_skipped.is_empty() {
                                    ui.collapsing(
                                        format!(
                                            "{} ({})",
                                            language.text("已跳过的片段", "Skipped segments"),
                                            self.subtitle_skipped.len()
                                        ),
                                        |ui| {
                                            egui::ScrollArea::vertical().max_height(180.0).show(
                                                ui,
                                                |ui| {
                                                    for (start, end, error) in
                                                        &self.subtitle_skipped
                                                    {
                                                        ui.label(format!(
                                                            "{}–{}",
                                                            fmt_time(*start),
                                                            fmt_time(*end)
                                                        ));
                                                        ui.small(language.error(error));
                                                    }
                                                },
                                            );
                                        },
                                    );
                                }
                                ui.checkbox(
                                    &mut self.subtitles_visible,
                                    language.text("显示字幕", "Show subtitles"),
                                );
                                if self.subtitle_job.is_some() {
                                    if ui
                                        .button(language.text("取消生成", "Cancel generation"))
                                        .clicked()
                                    {
                                        act = Act::CancelSubtitles;
                                        ui.close();
                                    }
                                } else if ui
                                    .add(egui::Button::new(if self.subtitles.is_empty() {
                                        language.text("生成 AI 字幕", "Generate AI subtitles")
                                    } else {
                                        language.text("重新生成字幕", "Regenerate subtitles")
                                    }))
                                    .on_hover_text(language.text(
                                        "将当前视频的音频分段发送到配置的模型服务",
                                        "Send audio chunks to the configured model service",
                                    ))
                                    .clicked()
                                {
                                    act = Act::GenerateSubtitles;
                                    ui.close();
                                }
                                if self.torrent_selected.is_some()&&!self.torrent_progress.finished {
                                    ui.small(language.text("只读取已下载音频；数据不足时等待，不抢占播放下载", "Uses downloaded audio only; waits for missing data without competing with playback"));
                                }
                                if ui
                                    .add_enabled(
                                        !self.subtitles.is_empty(),
                                        egui::Button::new(
                                            language.text("导出 SRT…", "Export SRT…"),
                                        ),
                                    )
                                    .clicked()
                                {
                                    act = Act::ExportSubtitles;
                                    ui.close();
                                }
                            },
                        );
                    }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if self.player.is_none() {
                            if ui
                                .selectable_value(
                                    &mut self.mode,
                                    UiMode::Music,
                                    language.text("音乐", "Music"),
                                )
                                .clicked()
                            {
                                self.set_active_queue(true);
                            }
                            if ui
                                .selectable_value(
                                    &mut self.mode,
                                    UiMode::Video,
                                    language.text("视频", "Video"),
                                )
                                .clicked()
                            {
                                self.set_active_queue(false);
                            }
                        }
                    });
                });
            });
        }

        // ---- bottom bar ----
        if a > 0.02 && self.player.is_some() && !music {
            let has_audio = self.player.as_ref().map(|p| p.has_audio()).unwrap_or(false);
            let (mut vol, mut muted) = (self.vol, self.muted);
            let mut scrubbing = self.scrubbing;
            let mut scrub = self.scrub;

            let bot_rect = Rect::from_min_max(
                pos2(screen.min.x, screen.max.y - BAR_BOTTOM),
                pos2(screen.max.x, screen.max.y),
            );
            painter.rect_filled(
                bot_rect,
                CornerRadius::same(0),
                SURFACE.gamma_multiply(a * 0.96),
            );
            let mut bui = root.new_child(
                UiBuilder::new()
                    .id_salt("bottombar")
                    .max_rect(bot_rect.shrink2(vec2(20.0, 17.0)))
                    .layout(Layout::top_down(Align::Min)),
            );
            bui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let r = icon_button(ui, 30.0, a, if playing { draw_pause } else { draw_play });
                if r.clicked() {
                    act = Act::TogglePlay;
                }
                let cur = if scrubbing { scrub } else { pos };
                ui.label(
                    RichText::new(format!("{} / {}", fmt_time(cur), fmt_time(dur)))
                        .monospace()
                        .color(Color32::from_white_alpha((225.0 * a) as u8))
                        .size(13.0),
                );
                let durmax = dur.max(0.0001);
                let shown = if scrubbing { scrub } else { pos };
                let frac = (shown / durmax).clamp(0.0, 1.0) as f32;
                let volume_slider = screen.width() >= 640.0;
                let sw = (ui.available_width()
                    - if has_audio {
                        if volume_slider { 170.0 } else { 78.0 }
                    } else {
                        40.0
                    })
                .max(30.0);
                let (srect, sresp) = hslider(ui, sw, 26.0, frac, 4.0, true, a);
                if sresp.dragged()
                    && let Some(pp) = ui.input(|i| i.pointer.hover_pos())
                {
                    let f = ((pp.x - srect.min.x) / srect.width()).clamp(0.0, 1.0);
                    scrub = f as f64 * dur;
                    scrubbing = true;
                }
                if sresp.drag_stopped() {
                    act = Act::Seek(scrub);
                    scrubbing = false;
                } else if sresp.clicked()
                    && let Some(pp) = ui.input(|i| i.pointer.hover_pos())
                {
                    let f = ((pp.x - srect.min.x) / srect.width()).clamp(0.0, 1.0);
                    act = Act::Seek(f as f64 * dur);
                }
                if has_audio {
                    let eff = if muted { 0.0 } else { vol };
                    let vr = icon_button(ui, 26.0, a, |p, rr, c| draw_volume(p, rr, c, muted, eff));
                    if vr.clicked() {
                        act = Act::ToggleMute;
                    }
                    if volume_slider {
                        let (vrect, vresp) = hslider(ui, 80.0, 26.0, eff, 3.5, false, a);
                        if (vresp.dragged() || vresp.clicked())
                            && let Some(pp) = ui.input(|i| i.pointer.hover_pos())
                        {
                            let f = ((pp.x - vrect.min.x) / vrect.width()).clamp(0.0, 1.0);
                            vol = f;
                            muted = false;
                            act = Act::Volume(f);
                        }
                    }
                }
                let fr = icon_button(ui, 28.0, a, draw_fullscreen);
                if fr.clicked() {
                    act = Act::ToggleFullscreen;
                }
            });

            self.vol = vol;
            self.muted = muted;
            self.scrub = scrub;
            self.scrubbing = scrubbing;
        }

        if music {
            self.music_view(root, screen, playback_rect, &mut act);
        }
        if sidebar_width > 0.0 {
            self.queue_sidebar(
                root,
                Rect::from_min_max(
                    pos2(playback_rect.max.x, screen.min.y + BAR_TOP),
                    pos2(
                        screen.max.x,
                        screen.max.y - if music { 108.0 } else { BAR_BOTTOM },
                    ),
                ),
                &mut act,
            );
        }

        // ---- empty state ----
        if self.player.is_none() {
            let center = if music {
                let content = Rect::from_min_max(
                    playback_rect.min + vec2(24.0, BAR_TOP + 20.0),
                    playback_rect.max - vec2(24.0, 126.0),
                );
                content.center()
            } else {
                playback_rect.center() + vec2(0.0, 12.0)
            };
            let mut eui = root.new_child(
                UiBuilder::new()
                    .id_salt("empty")
                    .max_rect(Rect::from_center_size(center, vec2(220.0, 56.0)))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            eui.spacing_mut().item_spacing.x = 28.0;
            if icon_button(&mut eui, 44.0, 1.0, draw_folder)
                .on_hover_text(language.text("打开媒体文件", "Open media files"))
                .clicked()
            {
                act = Act::Open;
            }
            if icon_button(&mut eui, 44.0, 1.0, draw_magnet)
                .on_hover_text(language.text("打开磁链", "Open magnet link"))
                .clicked()
            {
                self.magnet_open = true;
            }
        }

        // ---- error ----
        if let Some(e) = self.error.clone() {
            let qq_file = self
                .source
                .as_deref()
                .is_some_and(crate::media_source::is_qq);
            let error_area = if qq_file { playback_rect } else { screen };
            let r_rect = Rect::from_center_size(
                pos2(
                    error_area.center().x,
                    screen.min.y + if qq_file { 140.0 } else { 70.0 },
                ),
                vec2(
                    560.0_f32.min(error_area.width() - 24.0),
                    if qq_file { 150.0 } else { 44.0 },
                ),
            );
            painter.rect_filled(
                r_rect,
                CornerRadius::same(6),
                Color32::from_rgba_unmultiplied(120, 20, 20, 210),
            );
            let mut rui = root.new_child(
                UiBuilder::new()
                    .id_salt("err")
                    .max_rect(r_rect.shrink(8.0))
                    .layout(Layout::top_down(Align::Center)),
            );
            rui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        language.text("错误", "Error"),
                        language.error(&e)
                    ))
                    .color(Color32::WHITE)
                    .size(13.0),
                );
                if qq_file {
                    ui.label(language.text("需要密钥的歌曲可导入对应的 .ekey 文本文件，仅用于本次播放。", "Import the song's .ekey text file when a key is required. Used for this playback only."));
                    if ui.add_enabled(self.qq_key_rx.is_none(), egui::Button::new(language.text("导入歌曲密钥并重试", "Import song key and retry"))).clicked() {
                        self.import_qq_key();
                    }
                }
            });
        }

        self.preferences(&ctx);
        self.magnet_dialog(&ctx);
        if let Some(player) = &self.player {
            let state = player.snapshot().state;
            if matches!(
                state,
                PlaybackState::Opening | PlaybackState::Seeking | PlaybackState::Buffering
            ) && self.torrent_selected.is_some()
                && !self.magnet_open
                && !self.settings_open
            {
                painter.text(
                    playback_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    language.text("正在缓冲…", "Buffering…"),
                    egui::FontId::proportional(18.0),
                    Color32::WHITE,
                );
            }
        }

        // ---- apply action ----
        if language != self.settings.language
            || previous_concurrency != self.settings.subtitle_concurrency
        {
            if language != self.settings.language {
                self.subtitle_job = None;
                self.subtitles.clear();
                self.subtitle_skipped.clear();
                self.subtitle_metrics.clear();
                self.subtitle_status = self
                    .settings
                    .language
                    .text(
                        "语言已更改，请重新生成字幕",
                        "Language changed. Generate subtitles in the new language.",
                    )
                    .into();
            }
            if let Err(error) = self.settings.save() {
                self.error = Some(format!(
                    "{}: {error}",
                    self.settings
                        .language
                        .text("保存设置失败", "Cannot save settings")
                ));
            }
            ctx.request_repaint();
        }
        match act {
            Act::TogglePlay => {
                if let Some(p) = &self.player {
                    p.toggle_play();
                }
            }
            Act::Seek(t) => {
                if let Some(job) = &self.subtitle_job {
                    job.prioritize(t);
                }
                if let Some(p) = &self.player {
                    self.pending_seek = Some((p.seek(t), t));
                }
            }
            Act::Volume(v) => {
                self.vol = v.clamp(0.0, 1.0);
                self.muted = false;
                if let Some(p) = &self.player {
                    p.set_volume(self.vol);
                }
            }
            Act::ToggleMute => {
                self.muted = !self.muted;
                if let Some(p) = &self.player {
                    p.set_volume(if self.muted { 0.0 } else { self.vol });
                }
            }
            Act::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Act::Open => {
                if !self.dialog_open {
                    self.append_dialog = false;
                    self.open_dialog();
                }
            }
            Act::AddFiles => {
                if !self.dialog_open {
                    self.append_dialog = true;
                    self.open_dialog();
                }
            }
            Act::Next => {
                if let Some(i) = self.queue.next_index() {
                    self.play_queue(i);
                }
            }
            Act::Previous => {
                if music && pos > 3.0 {
                    if let Some(p) = &self.player {
                        self.pending_seek = Some((p.seek(0.0), 0.0));
                    }
                } else if let Some(i) = self.queue.previous_index() {
                    self.play_queue(i);
                }
            }
            Act::QueuePlay(i) => self.play_queue(i),
            Act::QueueRemove(i) => {
                if self.queue.current == Some(i) {
                    self.player = None;
                    self.media_info = None;
                    self.artwork = None;
                    self.video.clear();
                    self.subtitle_job = None;
                    self.subtitles.clear();
                    self.source = None;
                    self.title.clear();
                }
                self.queue.remove(i);
                self.persist_queue();
            }
            Act::GenerateSubtitles => {
                let subtitle_source = self
                    .torrent_selected
                    .as_ref()
                    .filter(|_| self.torrent_progress.finished)
                    .map(|file| file.path.clone())
                    .or_else(|| {
                        self.source.as_ref().map(|source| {
                            if self.torrent_selected.is_some() {
                                PathBuf::from(format!("{}/cached", source.to_string_lossy()))
                            } else {
                                source.clone()
                            }
                        })
                    });
                let cache = if self.torrent_selected.is_some() && !self.torrent_progress.finished {
                    self.torrent_job
                        .as_ref()
                        .map(|job| job.cache_misses.clone())
                } else {
                    None
                };
                if let Some(path) = &subtitle_source {
                    match subtitles::LlmConfig::load_with(&self.settings.llm).and_then(|config| {
                        Job::start_with_cache(
                            path.clone(),
                            subtitles::Options {
                                language: self.settings.language,
                                concurrency: self.settings.subtitle_concurrency,
                                ..Default::default()
                            },
                            pos,
                            config,
                            cache,
                        )
                    }) {
                        Ok(job) => {
                            self.subtitle_job = Some(job);
                            self.subtitles.clear();
                            self.subtitle_skipped.clear();
                            self.subtitles_visible = true;
                            self.subtitle_status =
                                language.text("正在准备音轨…", "Preparing audio…").into();
                            self.subtitle_metrics.clear();
                            self.error = None;
                        }
                        Err(error) => {
                            self.error = Some(format!(
                                "{}: {error}",
                                language
                                    .text("无法启动字幕任务", "Cannot start subtitle generation")
                            ))
                        }
                    }
                }
            }
            Act::CancelSubtitles => {
                self.subtitle_job = None;
                self.subtitle_status = language
                    .text(
                        "已取消生成，已完成的字幕仍可导出",
                        "Cancelled. Completed subtitles can still be exported.",
                    )
                    .into();
            }
            Act::ExportSubtitles => {
                let name = self
                    .source
                    .as_ref()
                    .map(|p| {
                        p.with_extension(if language == Language::Chinese {
                            "zh-CN.srt"
                        } else {
                            "en-US.srt"
                        })
                    })
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "subtitles.srt".into());
                subtitles::export_dialog(
                    &self.subtitles,
                    name,
                    self.subtitle_save_tx.clone(),
                    language,
                );
            }
            Act::None => {}
        }

        // ---- stats ----
        if self.stats && self.last_stats.elapsed().as_secs_f64() >= 1.0 {
            self.last_stats = Instant::now();
            if let Some(p) = &self.player {
                eprintln!(
                    "[stats] now={:.2} pos={:.2} dur={:.2} frames={} queued={} pend={:?} playing={} audio={}",
                    p.sync_time(),
                    p.position(),
                    p.duration(),
                    self.frames,
                    p.queued(),
                    p.pending_pts(),
                    p.is_playing(),
                    p.has_audio()
                );
            }
        }

        // ---- repaint pacing ----
        let transitioning = self.player.as_ref().is_some_and(|p| {
            matches!(
                p.snapshot().state,
                PlaybackState::Opening | PlaybackState::Seeking
            )
        });
        let delay = if music {
            if playing { 0.1 } else { 0.25 }
        } else if (a - target).abs() > 0.001 {
            1.0 / 60.0
        } else if transitioning {
            0.01
        } else if playing {
            let next = self
                .player
                .as_ref()
                .and_then(|p| p.pending_pts())
                .map(|pts| (pts - pos).clamp(0.0, 0.05))
                .unwrap_or(0.01);
            next.min(0.05)
        } else if a > 0.02 && a < 1.0 {
            0.03
        } else {
            0.25
        };
        ctx.request_repaint_after(Duration::from_secs_f64(delay));
    }
}

fn paint_subtitle(painter: &Painter, video: Rect, screen: Rect, text: &str) {
    let width = video.width() * 0.86;
    let mut font_size = (video.height() * 0.044).clamp(18.0, 42.0);
    let mut lines: Vec<_> = text
        .lines()
        .map(|line| {
            painter.layout_no_wrap(
                line.into(),
                egui::FontId::proportional(font_size),
                Color32::WHITE,
            )
        })
        .collect();
    let widest = lines.iter().map(|g| g.size().x).fold(0.0f32, f32::max);
    if widest > width {
        font_size *= width / widest;
        lines = text
            .lines()
            .map(|line| {
                painter.layout_no_wrap(
                    line.into(),
                    egui::FontId::proportional(font_size),
                    Color32::WHITE,
                )
            })
            .collect();
    }
    let gap = font_size * 0.12;
    let height =
        lines.iter().map(|g| g.size().y).sum::<f32>() + gap * lines.len().saturating_sub(1) as f32;
    // Reserve toolbar space even while hidden so subtitles never bounce.
    let bottom =
        (video.max.y - (video.height() * 0.05).max(18.0)).min(screen.max.y - BAR_BOTTOM - 14.0);
    let mut y = bottom - height;
    for line in lines {
        let origin = pos2(video.center().x - line.size().x * 0.5, y);
        for offset in [
            vec2(-1.5, 0.0),
            vec2(1.5, 0.0),
            vec2(0.0, -1.5),
            vec2(0.0, 1.5),
            vec2(-1.0, -1.0),
            vec2(1.0, -1.0),
            vec2(-1.0, 1.0),
            vec2(1.0, 1.0),
        ] {
            painter.galley_with_override_text_color(
                origin + offset,
                line.clone(),
                Color32::from_black_alpha(230),
            );
        }
        y += line.size().y + gap;
        painter.galley(origin, line, Color32::WHITE);
    }
}

fn hslider(
    ui: &mut Ui,
    width: f32,
    height: f32,
    frac: f32,
    bar: f32,
    handle_on_hover: bool,
    alpha: f32,
) -> (Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let hov = resp.hovered() || resp.dragged();
    let th = if hov && handle_on_hover {
        bar * 1.8
    } else {
        bar
    };
    let cy = rect.center().y;
    let p = ui.painter();
    let cr = CornerRadius::same((th * 0.5).clamp(0.0, 8.0) as u8);
    p.rect_filled(
        Rect::from_min_max(
            pos2(rect.min.x, cy - th * 0.5),
            pos2(rect.max.x, cy + th * 0.5),
        ),
        cr,
        Color32::from_white_alpha((70.0 * alpha) as u8),
    );
    let fx = rect.min.x + rect.width() * frac;
    let fill = ACCENT.gamma_multiply(alpha);
    p.rect_filled(
        Rect::from_min_max(pos2(rect.min.x, cy - th * 0.5), pos2(fx, cy + th * 0.5)),
        cr,
        fill,
    );
    if hov && handle_on_hover {
        p.circle_filled(pos2(fx, cy), th * 0.95, fill);
    }
    (rect, resp)
}

fn icon_button(
    ui: &mut Ui,
    size: f32,
    alpha: f32,
    draw: impl FnOnce(&Painter, Rect, Color32),
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(
            rect.expand(3.0),
            CornerRadius::same(6),
            Color32::from_white_alpha((38.0 * alpha) as u8),
        );
    }
    let base = if resp.hovered() { 255.0 } else { 225.0 };
    let c = Color32::from_white_alpha((base * alpha) as u8);
    draw(ui.painter(), rect.shrink(size * 0.24), c);
    resp
}

fn draw_play(p: &Painter, r: Rect, c: Color32) {
    let rr = r.width().min(r.height()) * 0.5;
    let cx = r.center().x;
    let cy = r.center().y;
    p.add(Shape::convex_polygon(
        vec![
            pos2(cx - rr * 0.7, cy - rr * 0.9),
            pos2(cx - rr * 0.7, cy + rr * 0.9),
            pos2(cx + rr * 0.7, cy),
        ],
        c,
        Stroke::NONE,
    ));
}

fn draw_pause(p: &Painter, r: Rect, c: Color32) {
    let bw = r.width() * 0.24;
    let bh = r.height() * 0.82;
    let cx = r.center().x;
    let cy = r.center().y;
    let off = r.width() * 0.19;
    p.rect_filled(
        Rect::from_center_size(pos2(cx - off, cy), vec2(bw, bh)),
        CornerRadius::same(1),
        c,
    );
    p.rect_filled(
        Rect::from_center_size(pos2(cx + off, cy), vec2(bw, bh)),
        CornerRadius::same(1),
        c,
    );
}

fn draw_prev(p: &Painter, r: Rect, c: Color32) {
    let rr = r.width().min(r.height()) * 0.5;
    let cx = r.center().x;
    let cy = r.center().y;
    p.rect_filled(
        Rect::from_min_max(
            pos2(cx - rr * 0.95, cy - rr * 0.85),
            pos2(cx - rr * 0.65, cy + rr * 0.85),
        ),
        CornerRadius::same(1),
        c,
    );
    p.add(Shape::convex_polygon(
        vec![
            pos2(cx + rr * 0.9, cy - rr * 0.85),
            pos2(cx + rr * 0.9, cy + rr * 0.85),
            pos2(cx - rr * 0.4, cy),
        ],
        c,
        Stroke::NONE,
    ));
}

fn draw_next(p: &Painter, r: Rect, c: Color32) {
    let rr = r.width().min(r.height()) * 0.5;
    let cx = r.center().x;
    let cy = r.center().y;
    p.rect_filled(
        Rect::from_min_max(
            pos2(cx + rr * 0.65, cy - rr * 0.85),
            pos2(cx + rr * 0.95, cy + rr * 0.85),
        ),
        CornerRadius::same(1),
        c,
    );
    p.add(Shape::convex_polygon(
        vec![
            pos2(cx - rr * 0.9, cy - rr * 0.85),
            pos2(cx - rr * 0.9, cy + rr * 0.85),
            pos2(cx + rr * 0.4, cy),
        ],
        c,
        Stroke::NONE,
    ));
}

fn disabled_icon_button(ui: &mut Ui, size: f32, draw: impl FnOnce(&Painter, Rect, Color32)) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    draw(
        ui.painter(),
        rect.shrink(size * 0.24),
        Color32::from_white_alpha(70),
    );
}

fn play_circle_button(ui: &mut Ui, size: f32, playing: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let fill = if resp.hovered() || resp.clicked() {
        ACCENT.gamma_multiply(1.25)
    } else {
        ACCENT
    };
    ui.painter().circle_filled(rect.center(), size * 0.5, fill);
    let icon = rect.shrink(size * 0.33);
    if playing {
        draw_pause(ui.painter(), icon, Color32::WHITE);
    } else {
        draw_play(ui.painter(), icon, Color32::WHITE);
    }
    resp
}

fn draw_repeat(p: &Painter, r: Rect, c: Color32, single: bool) {
    let s = Stroke::new(2.0, c);
    let aw = r.width() * 0.16;
    let mx = r.center().x;
    let (y0, y1) = (r.min.y, r.max.y);
    p.line_segment([pos2(r.min.x, y0), pos2(mx - aw * 1.3, y0)], s);
    p.line_segment([pos2(mx + aw * 1.3, y0), pos2(r.max.x, y0)], s);
    p.line_segment([pos2(r.min.x, y1), pos2(mx - aw * 1.3, y1)], s);
    p.line_segment([pos2(mx + aw * 1.3, y1), pos2(r.max.x, y1)], s);
    p.line_segment([pos2(r.min.x, y0), pos2(r.min.x, y1)], s);
    p.line_segment([pos2(r.max.x, y0), pos2(r.max.x, y1)], s);
    p.add(Shape::convex_polygon(
        vec![
            pos2(mx - aw * 0.7, y0 - aw),
            pos2(mx - aw * 0.7, y0 + aw),
            pos2(mx + aw, y0),
        ],
        c,
        Stroke::NONE,
    ));
    p.add(Shape::convex_polygon(
        vec![
            pos2(mx + aw * 0.7, y1 - aw),
            pos2(mx + aw * 0.7, y1 + aw),
            pos2(mx - aw, y1),
        ],
        c,
        Stroke::NONE,
    ));
    if single {
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "1",
            egui::FontId::proportional(r.height() * 0.66),
            c,
        );
    }
}

fn draw_trash(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(2.0, c);
    let lid = r.min.y + r.height() * 0.26;
    p.line_segment(
        [
            pos2(r.center().x - r.width() * 0.16, r.min.y + r.height() * 0.1),
            pos2(r.center().x + r.width() * 0.16, r.min.y + r.height() * 0.1),
        ],
        s,
    );
    p.line_segment([pos2(r.min.x, lid), pos2(r.max.x, lid)], s);
    p.line_segment(
        [
            pos2(r.min.x + r.width() * 0.14, lid),
            pos2(r.min.x + r.width() * 0.24, r.max.y),
        ],
        s,
    );
    p.line_segment(
        [
            pos2(r.max.x - r.width() * 0.14, lid),
            pos2(r.max.x - r.width() * 0.24, r.max.y),
        ],
        s,
    );
    p.line_segment(
        [
            pos2(r.min.x + r.width() * 0.24, r.max.y),
            pos2(r.max.x - r.width() * 0.24, r.max.y),
        ],
        s,
    );
    let ribs = Stroke::new(1.5, c);
    p.line_segment(
        [
            pos2(r.center().x - r.width() * 0.13, lid + r.height() * 0.14),
            pos2(r.center().x - r.width() * 0.11, r.max.y - r.height() * 0.12),
        ],
        ribs,
    );
    p.line_segment(
        [
            pos2(r.center().x + r.width() * 0.13, lid + r.height() * 0.14),
            pos2(r.center().x + r.width() * 0.11, r.max.y - r.height() * 0.12),
        ],
        ribs,
    );
}

fn draw_close(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(2.0, c);
    p.line_segment([r.left_top(), r.right_bottom()], s);
    p.line_segment([r.left_bottom(), r.right_top()], s);
}

fn draw_folder(p: &Painter, r: Rect, c: Color32) {
    let tab_w = r.width() * 0.38;
    let tab_y = r.min.y + r.height() * 0.18;
    let top = r.min.y + r.height() * 0.38;
    let pts = vec![
        pos2(r.min.x, r.max.y),
        pos2(r.min.x, tab_y),
        pos2(r.min.x + tab_w, tab_y),
        pos2(r.min.x + tab_w + r.width() * 0.1, top),
        pos2(r.max.x, top),
        pos2(r.max.x, r.max.y),
        pos2(r.min.x, r.max.y),
    ];
    p.add(Shape::closed_line(pts, Stroke::new(2.0, c)));
}

fn draw_magnet(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(2.0, c);
    let cx = r.center().x;
    let cy = r.center().y + r.height() * 0.08;
    let rad = r.width() * 0.3;
    let mut pts = Vec::new();
    for i in 0..=12 {
        let ang = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 12.0;
        pts.push(pos2(cx + ang.cos() * rad, cy + ang.sin() * rad));
    }
    p.add(Shape::line(pts, s));
    p.line_segment([pos2(cx - rad, cy), pos2(cx - rad, r.max.y)], s);
    p.line_segment([pos2(cx + rad, cy), pos2(cx + rad, r.max.y)], s);
    p.line_segment(
        [
            pos2(cx - rad, r.max.y - r.height() * 0.22),
            pos2(cx - rad, r.max.y),
        ],
        Stroke::new(3.5, c),
    );
    p.line_segment(
        [
            pos2(cx + rad, r.max.y - r.height() * 0.22),
            pos2(cx + rad, r.max.y),
        ],
        Stroke::new(3.5, c),
    );
}

fn draw_settings(p: &Painter, r: Rect, c: Color32) {
    let center = r.center();
    let rad = r.width().min(r.height()) * 0.5;
    p.circle_stroke(center, rad * 0.58, Stroke::new(2.0, c));
    for i in 0..8 {
        let ang = i as f32 * std::f32::consts::TAU / 8.0;
        let (sx, sy) = (ang.cos(), ang.sin());
        p.line_segment(
            [
                pos2(center.x + sx * rad * 0.58, center.y + sy * rad * 0.58),
                pos2(center.x + sx * rad, center.y + sy * rad),
            ],
            Stroke::new(2.0, c),
        );
    }
    p.circle_filled(center, rad * 0.2, c);
}

fn draw_add(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(2.0, c);
    let cx = r.center().x;
    let cy = r.center().y;
    let d = r.width().min(r.height()) * 0.42;
    p.line_segment([pos2(cx - d, cy), pos2(cx + d, cy)], s);
    p.line_segment([pos2(cx, cy - d), pos2(cx, cy + d)], s);
}

fn draw_queue(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(2.0, c);
    let (w, h) = (r.width(), r.height());
    p.line_segment(
        [
            pos2(r.min.x, r.min.y + h * 0.2),
            pos2(r.max.x, r.min.y + h * 0.2),
        ],
        s,
    );
    p.line_segment(
        [
            pos2(r.min.x, r.min.y + h * 0.5),
            pos2(r.max.x, r.min.y + h * 0.5),
        ],
        s,
    );
    p.line_segment(
        [
            pos2(r.min.x, r.min.y + h * 0.8),
            pos2(r.min.x + w * 0.55, r.min.y + h * 0.8),
        ],
        s,
    );
    p.add(Shape::convex_polygon(
        vec![
            pos2(r.min.x + w * 0.7, r.min.y + h * 0.62),
            pos2(r.min.x + w * 0.7, r.max.y),
            pos2(r.max.x, r.min.y + h * 0.81),
        ],
        c,
        Stroke::NONE,
    ));
}

fn vslider(
    ui: &mut Ui,
    width: f32,
    height: f32,
    frac: f32,
    bar: f32,
    alpha: f32,
) -> (Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let hov = resp.hovered() || resp.dragged();
    let th = if hov { bar * 1.6 } else { bar };
    let cx = rect.center().x;
    let p = ui.painter();
    let cr = CornerRadius::same((th * 0.5).clamp(0.0, 8.0) as u8);
    p.rect_filled(
        Rect::from_min_max(
            pos2(cx - th * 0.5, rect.min.y),
            pos2(cx + th * 0.5, rect.max.y),
        ),
        cr,
        Color32::from_white_alpha((70.0 * alpha) as u8),
    );
    let fy = rect.max.y - rect.height() * frac;
    let fill = ACCENT.gamma_multiply(alpha);
    p.rect_filled(
        Rect::from_min_max(pos2(cx - th * 0.5, fy), pos2(cx + th * 0.5, rect.max.y)),
        cr,
        fill,
    );
    if hov {
        p.circle_filled(pos2(cx, fy), th * 0.95, fill);
    }
    (rect, resp)
}

fn draw_fullscreen(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(2.0, c);
    let d = r.width().min(r.height()) * 0.34;
    let lt = r.left_top();
    let rt = r.right_top();
    let lb = r.left_bottom();
    let rb = r.right_bottom();
    p.line_segment([lt, pos2(lt.x + d, lt.y)], s);
    p.line_segment([lt, pos2(lt.x, lt.y + d)], s);
    p.line_segment([rt, pos2(rt.x - d, rt.y)], s);
    p.line_segment([rt, pos2(rt.x, rt.y + d)], s);
    p.line_segment([lb, pos2(lb.x + d, lb.y)], s);
    p.line_segment([lb, pos2(lb.x, lb.y - d)], s);
    p.line_segment([rb, pos2(rb.x - d, rb.y)], s);
    p.line_segment([rb, pos2(rb.x, rb.y - d)], s);
}

fn draw_volume(p: &Painter, r: Rect, c: Color32, muted: bool, level: f32) {
    let cy = r.center().y;
    let h = r.height();
    let w = r.width();
    let bx = r.min.x + w * 0.06;
    p.rect_filled(
        Rect::from_center_size(pos2(bx + w * 0.07, cy), vec2(w * 0.14, h * 0.28)),
        CornerRadius::same(1),
        c,
    );
    p.add(Shape::convex_polygon(
        vec![
            pos2(bx + w * 0.13, cy - h * 0.14),
            pos2(bx + w * 0.13, cy + h * 0.14),
            pos2(bx + w * 0.34, cy + h * 0.38),
            pos2(bx + w * 0.34, cy - h * 0.38),
        ],
        c,
        Stroke::NONE,
    ));
    let s = Stroke::new(1.8, c);
    if muted {
        let x0 = pos2(bx + w * 0.46, cy - h * 0.20);
        let x1 = pos2(bx + w * 0.74, cy + h * 0.20);
        p.line_segment([x0, x1], s);
        p.line_segment([pos2(x0.x, x1.y), pos2(x1.x, x0.y)], s);
    } else {
        if level > 0.02 {
            let rx = bx + w * 0.46;
            p.line_segment([pos2(rx, cy - h * 0.16), pos2(rx + w * 0.07, cy)], s);
            p.line_segment([pos2(rx + w * 0.07, cy), pos2(rx, cy + h * 0.16)], s);
        }
        if level > 0.5 {
            let rx = bx + w * 0.60;
            p.line_segment([pos2(rx, cy - h * 0.26), pos2(rx + w * 0.09, cy)], s);
            p.line_segment([pos2(rx + w * 0.09, cy), pos2(rx, cy + h * 0.26)], s);
        }
    }
}

fn setup_fonts(ctx: &egui::Context) {
    const CANDIDATES: [&str; 4] = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
    ];
    let Some(bytes) = CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        fam.push("cjk".to_owned());
    }
    if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        fam.push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn fmt_time(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "00:00".to_owned();
    }
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
