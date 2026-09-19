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
mod design;
mod preferences;
use design::{ACCENT, BACKGROUND, MUTED, SURFACE};

const BAR_BOTTOM: f32 = 68.0;
const BAR_TOP: f32 = 60.0;
const TOOLBAR_HIDE_DELAY: Duration = Duration::from_millis(400);

enum Act {
    None,
    TogglePlay,
    Seek(f64),
    Volume(f32),
    ToggleMute,
    ToggleFullscreen,
    Open,
    GenerateSubtitles,
    CancelSubtitles,
    ExportSubtitles,
}

pub struct App {
    settings: Settings,
    settings_open: bool,
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
    open_tx: Sender<Option<PathBuf>>,
    open_rx: Receiver<Option<PathBuf>>,
    dialog_open: bool,
    scrubbing: bool,
    scrub: f64,
    pending_seek: Option<(u64, f64)>,
    vol: f32,
    muted: bool,
    fullscreen: bool,
    ui_alpha: f32,
    last_toolbar_hover: Option<Instant>,
    error: Option<String>,
    frames: u64,
    last_stats: Instant,
    stats: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<String>) -> Self {
        setup_fonts(&cc.egui_ctx);
        design::apply(&cc.egui_ctx);
        let settings = Settings::load();
        let mut defaults = subtitles::LlmConfig::defaults().unwrap_or_default();
        let env_key_available = !defaults.api_key.is_empty();
        defaults.api_key.clear();
        let (open_tx, open_rx) = unbounded();
        let (subtitle_save_tx, subtitle_save_rx) = unbounded();
        let mut app = Self {
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
            scrubbing: false,
            scrub: 0.0,
            pending_seek: None,
            vol: 1.0,
            muted: false,
            fullscreen: false,
            ui_alpha: 0.0,
            last_toolbar_hover: None,
            error: None,
            frames: 0,
            last_stats: Instant::now(),
            stats: std::env::var_os("REPLAYER_STATS").is_some(),
        };
        if let Some(p) = initial {
            app.load(p);
        }
        app
    }

    fn load(&mut self, path: String) {
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
        match Player::open(path) {
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
                .set_title(language.text("选择视频文件", "Choose a video"))
                .add_filter(
                    language.text("视频", "Video"),
                    &[
                        "mp4", "mkv", "webm", "mov", "m4v", "avi", "ts", "m2ts", "flv", "wmv",
                        "mpg", "mpeg", "3gp", "ogv",
                    ],
                )
                .pick_file();
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
        if let Some(path) = ctx.input(|i| {
            i.raw
                .dropped_files
                .first()
                .map(|file| file.path().to_path_buf())
        }) {
            self.load(path.to_string_lossy().into_owned());
        }

        if let Ok(picked) = self.open_rx.try_recv() {
            self.dialog_open = false;
            if let Some(p) = picked {
                self.load(p.to_string_lossy().into_owned());
            }
        }

        if let Some(player) = &self.player {
            while let Some(event) = player.poll_event() {
                match event {
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
        let mut subtitle_done = false;
        if let Some(job) = &self.subtitle_job {
            while let Ok(event) = job.events.try_recv() {
                match event {
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
        let show = self.player.is_none()
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
            if self.player.is_some() {
                Color32::BLACK
            } else {
                BACKGROUND
            },
        );
        let mut video_rect = screen;
        if let Some((texture, ar)) = self.video.current() {
            let mut w = screen.width();
            let mut h = w / ar;
            if h > screen.height() {
                h = screen.height();
                w = h * ar;
            }
            let vr = Rect::from_center_size(screen.center(), vec2(w, h));
            video_rect = vr;
            painter.image(
                texture,
                vr,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // Subtitles remain visible when the toolbars are hidden and follow seeks.
        if self.subtitles_visible
            && let Some(text) = subtitles::active(&self.subtitles, pos)
        {
            paint_subtitle(&painter, video_rect, screen, text);
        }

        // ---- video click area ----
        let mut click_rect = screen;
        if self.player.is_some() {
            click_rect.max.y = screen.max.y - BAR_BOTTOM;
            click_rect.min.y = screen.min.y + BAR_TOP;
        }
        let vresp = root.interact(click_rect, Id::new("video_click"), Sense::click());

        let mut act = Act::None;
        if self.player.is_some() && !self.settings_open {
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
            } else {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
        }
        if self.player.is_some() && !self.settings_open && !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(Key::Space)) {
                act = Act::TogglePlay;
            }
            if ctx.input(|i| i.key_pressed(Key::F)) {
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
            painter.rect_filled(
                top_rect,
                CornerRadius::same(0),
                SURFACE.gamma_multiply(a * 0.96),
            );
            let mut tui = root.new_child(
                UiBuilder::new()
                    .id_salt("topbar")
                    .max_rect(top_rect.shrink2(vec2(20.0, 12.0)))
                    .layout(Layout::top_down(Align::Min)),
            );
            let title = if self.player.is_none() {
                "replayer".into()
            } else {
                self.title.clone()
            };
            tui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.player.is_some()
                        && ui
                            .add(open_button(a, language))
                            .on_hover_text(language.text("打开视频文件", "Open a video"))
                            .clicked()
                    {
                        act = Act::Open;
                    }
                    if ui.button(language.text("设置", "Settings")).clicked() {
                        self.settings_open = true;
                        self.llm_draft = self.settings.llm.clone();
                        self.config_message.clear();
                    }
                    if self.player.is_some() {
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
                                    .button(if self.subtitles.is_empty() {
                                        language.text("生成 AI 字幕", "Generate AI subtitles")
                                    } else {
                                        language.text("重新生成字幕", "Regenerate subtitles")
                                    })
                                    .on_hover_text(language.text(
                                        "将当前视频的音频分段发送到配置的模型服务",
                                        "Send audio chunks to the configured model service",
                                    ))
                                    .clicked()
                                {
                                    act = Act::GenerateSubtitles;
                                    ui.close();
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
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(title)
                                    .color(Color32::from_white_alpha((230.0 * a) as u8))
                                    .strong()
                                    .size(14.0),
                            )
                            .truncate(),
                        );
                    });
                });
            });
        }

        // ---- bottom bar ----
        if a > 0.02 && self.player.is_some() {
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

        // ---- empty state ----
        if self.player.is_none() {
            let e_rect = Rect::from_center_size(
                screen.center() + vec2(0.0, 12.0),
                vec2(screen.width().min(460.0) - 32.0, 270.0),
            );
            let mut eui = root.new_child(
                UiBuilder::new()
                    .id_salt("empty")
                    .max_rect(e_rect)
                    .layout(Layout::top_down(Align::Center)),
            );
            eui.vertical_centered(|ui| {
                let (logo, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                ui.painter().rect_filled(logo, 20, SURFACE);
                draw_play(ui.painter(), logo.shrink(19.0), ACCENT);
                ui.add_space(20.0);
                ui.label(
                    RichText::new(language.text("让画面成为主角", "Make room for the picture"))
                        .size(28.0)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(language.text(
                        "本地播放 · AI 字幕 · 专注观看",
                        "Local playback · AI subtitles · Just watch",
                    ))
                    .color(MUTED)
                    .size(14.0),
                );
                ui.add_space(26.0);
                if ui
                    .add(open_button(1.0, language).min_size(vec2(160.0, 42.0)))
                    .clicked()
                {
                    act = Act::Open;
                }
                ui.add_space(14.0);
                ui.label(
                    RichText::new(language.text("或将视频拖到这里", "Or drop a video here"))
                        .color(MUTED)
                        .size(13.0),
                );
            });
        }

        // ---- error ----
        if let Some(e) = self.error.clone() {
            let r_rect = Rect::from_center_size(
                pos2(screen.center().x, screen.min.y + 70.0),
                vec2(420.0, 44.0),
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
            });
        }

        self.preferences(&ctx);

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
            Act::Open => self.open_dialog(),
            Act::GenerateSubtitles => {
                if let Some(path) = &self.source {
                    match subtitles::LlmConfig::load_with(&self.settings.llm).and_then(|config| {
                        Job::start_at(
                            path.clone(),
                            subtitles::Options {
                                language: self.settings.language,
                                concurrency: self.settings.subtitle_concurrency,
                                ..Default::default()
                            },
                            pos,
                            config,
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
        let delay = if (a - target).abs() > 0.001 {
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

fn open_button(a: f32, language: Language) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(language.text("打开视频", "Open video"))
            .color(Color32::from_white_alpha((235.0 * a) as u8))
            .size(13.0),
    )
    .fill(ACCENT.gamma_multiply(a))
    .corner_radius(CornerRadius::same(8))
    .min_size(vec2(90.0, 34.0))
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
            pos2(cx - rr * 0.55, cy - rr * 0.9),
            pos2(cx - rr * 0.55, cy + rr * 0.9),
            pos2(cx + rr * 0.9, cy),
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
