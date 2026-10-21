use super::*;
use design::{LINE, SELECTED, TEXT};
use library::Record;

mod home;
mod media;
mod subtitle_menu;
mod transport;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Home,
    Music,
    Video,
    Comic,
    Favorites,
    Recent,
    Downloads,
    Settings,
    Listening,
    Playing,
}
#[derive(Default)]
pub(super) struct State {
    pub page: Page,
    query: String,
    music_tab: usize,
    pub listening_tab: usize,
    filter: usize,
    pub resume: Option<(String, f64)>,
    downloads: Vec<String>,
}

#[derive(Clone, Copy)]
enum Glyph {
    Home,
    Music,
    Video,
    Book,
    Heart,
    Clock,
    Download,
    Settings,
}

impl App {
    pub(super) fn navigate(&mut self, page: Page) {
        self.update_library();
        self.library.flush();
        self.comic.flush();
        if matches!(page, Page::Comic | Page::Home | Page::Music | Page::Video)
            && !self.is_music()
            && let Some(player) = &self.player
        {
            player.set_playing(false);
        }
        if page == Page::Downloads {
            self.workspace.downloads = std::fs::read_dir(self.settings.music.directory())
                .into_iter()
                .flatten()
                .flatten()
                .take(2000)
                .map(|e| e.path().to_string_lossy().into_owned())
                .filter(|p| crate::media::supported(p))
                .collect();
        }
        if page == Page::Settings && !self.settings_open {
            self.llm_draft = self.settings.llm.clone();
            self.online_music.draft = self.settings.music.clone();
            self.config_message.clear();
        }
        self.settings_open = page == Page::Settings;
        self.workspace.page = page;
        self.workspace.query.clear();
    }

    pub(super) fn workspace_ui(&mut self, root: &mut Ui) {
        let ctx = root.ctx().clone();
        let screen = root.max_rect();
        let language = self.settings.language;
        if self.settings_open && self.workspace.page != Page::Settings {
            self.navigate(Page::Settings);
        }
        let mut act = Act::None;
        if ctx.input(|i| i.key_pressed(Key::Escape))
            && !(self.workspace.page == Page::Comic && self.comic.reading())
        {
            if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else if self.volume_open {
                self.volume_open = false;
            } else if self.online_music.open {
                self.online_music.open = false;
            } else if self.magnet_open {
                self.magnet_open = false;
            } else if self.workspace.page == Page::Listening {
                self.navigate(Page::Music);
            } else if self.workspace.page == Page::Settings {
                self.navigate(Page::Home);
            } else if self.queue_open {
                self.queue_open = false;
            }
        }
        if !ctx.egui_wants_keyboard_input()
            && !self.magnet_open
            && !self.online_music.open
            && !(self.workspace.page == Page::Comic && self.comic.reading())
            && let Some(player) = &self.player
        {
            if ctx.input(|i| i.key_pressed(Key::Space)) {
                act = Act::TogglePlay;
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
                act = Act::Seek((player.position() - 5.0).max(0.0));
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
                act = Act::Seek((player.position() + 5.0).min(player.duration()));
            }
        }
        root.painter().rect_filled(screen, 0, BACKGROUND);
        let reader = self.workspace.page == Page::Comic && self.comic.reading();
        let listening = self.workspace.page == Page::Listening;
        let footer_h = if reader {
            0.0
        } else {
            transport::MUSIC_BAR_HEIGHT
        };
        let nav_w = if reader || listening {
            0.0
        } else if screen.width() < 1000.0 {
            76.0
        } else {
            194.0
        };
        let queue_w = if !reader && self.queue_open {
            280.0_f32.min(screen.width() * 0.36)
        } else {
            0.0
        };
        let content = Rect::from_min_max(
            pos2(screen.min.x + nav_w, screen.min.y),
            pos2(screen.max.x - queue_w, screen.max.y - footer_h),
        );
        if nav_w > 0.0 {
            self.navigation(
                root,
                Rect::from_min_size(screen.min, vec2(nav_w, screen.height() - footer_h)),
            );
        }
        if reader {
            let mut ui = child(root, "immersive-comic", content);
            self.comic
                .ui(&mut ui, &mut self.settings, &mut self.settings_open);
        } else if listening {
            let mut header = child(
                root,
                "listening-header",
                Rect::from_min_size(
                    content.min + vec2(22.0, 8.0),
                    vec2(
                        (content.width() - 56.0 - window::CONTROLS_WIDTH).max(180.0),
                        36.0,
                    ),
                ),
            );
            header.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let (mark, _) = ui.allocate_exact_size(vec2(23.0, 26.0), Sense::hover());
                draw_play(ui.painter(), mark.shrink(3.0), ACCENT);
                ui.label(RichText::new("replayer").size(20.0).strong());
                ui.separator();
                if icon_button(ui, 28.0, 1.0, draw_back)
                    .on_hover_text(language.text("返回音乐库", "Back to music library"))
                    .clicked()
                {
                    self.navigate(Page::Music);
                }
                ui.label(language.text("音乐库", "Music library"));
            });
            let labels = [
                language.text("歌词", "Lyrics"),
                language.text("信息", "Info"),
            ];
            let width = design::segmented_width(root, &labels);
            let mut tabs = child(
                root,
                "listening-tabs",
                Rect::from_min_size(
                    pos2(content.max.x - width - 24.0, content.min.y + 64.0),
                    vec2(width, 34.0),
                ),
            );
            design::segmented(&mut tabs, &mut self.workspace.listening_tab, &labels);
            self.music_view(root, screen, content, &mut act);
        } else {
            let mut top = child(
                root,
                "workspace-header",
                Rect::from_min_size(
                    content.min + vec2(28.0, 12.0),
                    vec2(
                        (content
                            .max
                            .x
                            .min(screen.max.x - window::CONTROLS_WIDTH - 12.0)
                            - content.min.x
                            - 56.0)
                            .max(120.0),
                        38.0,
                    ),
                ),
            );
            // TextEdit paints its frame and hover stroke outside its allocation.
            // Keep that decoration inside the caption's clip, including its top.
            top.set_clip_rect(Rect::from_min_max(
                content.min + vec2(24.0, 4.0),
                pos2(
                    (screen.max.x - window::CONTROLS_WIDTH - 8.0).min(content.max.x),
                    content.min.y + 60.0,
                ),
            ));
            top.horizontal(|ui| {
                let width = ui.available_width().clamp(100.0, 530.0);
                let input = ui.add(
                    egui::TextEdit::singleline(&mut self.workspace.query)
                        .hint_text(language.text("搜索媒体、音乐或漫画", "Search your media"))
                        .desired_width(width)
                        .margin(egui::Margin {
                            left: 36,
                            right: 12,
                            top: 9,
                            bottom: 9,
                        }),
                );
                draw_search(
                    ui.painter(),
                    Rect::from_center_size(
                        pos2(input.rect.min.x + 18.0, input.rect.center().y),
                        Vec2::splat(15.0),
                    ),
                    MUTED,
                );
                if input.lost_focus()
                    && ui.input(|i| i.key_pressed(Key::Enter))
                    && self.workspace.page == Page::Comic
                {
                    self.comic.search_for(self.workspace.query.clone());
                }
            });
            root.painter().line_segment(
                [
                    pos2(content.min.x, content.min.y + 64.0),
                    pos2(content.max.x, content.min.y + 64.0),
                ],
                Stroke::new(1.0, LINE),
            );
            let body = Rect::from_min_max(
                content.min + vec2(28.0, 84.0),
                content.max - vec2(28.0, 14.0),
            );
            let mut ui = child(root, "workspace-content", body);
            match self.workspace.page {
                Page::Home => self.home_page(&mut ui),
                Page::Comic => {
                    if !self.comic.detail_open() {
                        page_heading(&mut ui, language.text("漫画", "Comics"), None);
                    }
                    self.comic
                        .ui(&mut ui, &mut self.settings, &mut self.settings_open);
                }
                Page::Settings => self.preferences_page(&mut ui),
                Page::Music | Page::Video | Page::Favorites | Page::Recent | Page::Downloads => {
                    self.media_library(&mut ui)
                }
                _ => {}
            }
            if matches!(
                self.workspace.page,
                Page::Home
                    | Page::Music
                    | Page::Video
                    | Page::Recent
                    | Page::Favorites
                    | Page::Downloads
            ) {
                let narrow = body.width() < 550.0;
                let width = if narrow { 40.0 } else { 134.0 };
                let mut actions = child(
                    root,
                    "page-open-action",
                    Rect::from_min_size(pos2(body.max.x - width, body.min.y), vec2(width, 38.0)),
                );
                actions.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let response = if narrow {
                        icon_button(ui, 32.0, 1.0, draw_folder)
                    } else {
                        primary(ui, language.text("＋ 打开文件", "+ Open files"))
                    };
                    if response
                        .on_hover_text(language.text("打开文件", "Open files"))
                        .clicked()
                    {
                        act = Act::Open;
                    }
                });
            }
        }
        if queue_w > 0.0 {
            self.queue_sidebar(
                root,
                Rect::from_min_max(
                    pos2(screen.max.x - queue_w, screen.min.y + 64.0),
                    pos2(screen.max.x, screen.max.y - footer_h),
                ),
                &mut act,
            );
        }
        if footer_h > 0.0 {
            self.mini_transport(root, screen, &mut act);
        }
        if self.loading && !reader {
            let mut ui = child(
                root,
                "opening-media",
                Rect::from_min_size(content.min + vec2(28.0, 64.0), vec2(260.0, 30.0)),
            );
            ui.horizontal(|ui| {
                ui.spinner();
                ui.small(language.text("正在打开媒体…", "Opening media…"));
            });
        }
        if let Some(error) = self.error.clone() {
            let r = Rect::from_min_size(
                pos2(content.min.x + 20.0, content.max.y - 74.0),
                vec2((content.width() - 40.0).max(160.0), 60.0),
            );
            root.painter()
                .rect_filled(r, 8, Color32::from_rgb(54, 35, 35));
            let mut ui = child(root, "workspace-error", r.shrink(10.0));
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(language.error(&error)).color(Color32::LIGHT_RED));
                if ui.small_button(language.text("关闭", "Dismiss")).clicked() {
                    self.error = None;
                }
                if self
                    .source
                    .as_deref()
                    .is_some_and(crate::media_source::is_qq)
                    && ui
                        .small_button(language.text("导入歌曲密钥", "Import song key"))
                        .clicked()
                {
                    self.import_qq_key();
                }
            });
        }
        self.window_frame(root, !reader || self.comic.toolbar_visible());
        self.online_music_window(&ctx);
        self.magnet_dialog(&ctx);
        self.apply_action(act, &ctx);
        if language != self.settings.language {
            self.comic.reload_images();
            self.subtitle_job = None;
            self.subtitles.clear();
            if let Err(e) = self.settings.save() {
                self.error = Some(e.to_string());
            }
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn navigation(&mut self, root: &mut Ui, rect: Rect) {
        let l = self.settings.language;
        let narrow = rect.width() < 100.0;
        root.painter().line_segment(
            [rect.right_top(), rect.right_bottom()],
            Stroke::new(1.0, LINE),
        );
        let mut ui = child(root, "main-navigation", rect.shrink2(vec2(12.0, 18.0)));
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
            draw_play(ui.painter(), r.shrink(3.0), ACCENT);
            if !narrow {
                ui.label(RichText::new("replayer").size(20.0).strong());
            }
        });
        ui.add_space(26.0);
        for (page, zh, en, glyph) in [
            (Page::Home, "首页", "Home", Glyph::Home),
            (Page::Music, "音乐", "Music", Glyph::Music),
            (Page::Video, "视频", "Video", Glyph::Video),
            (Page::Comic, "漫画", "Comics", Glyph::Book),
        ] {
            if nav_item(
                &mut ui,
                l.text(zh, en),
                glyph,
                self.workspace.page == page,
                narrow,
            )
            .clicked()
            {
                self.navigate(page);
            }
        }
        ui.add_space(18.0);
        ui.separator();
        ui.add_space(12.0);
        if !narrow {
            ui.label(
                RichText::new(l.text("我的资料库", "My library"))
                    .small()
                    .color(MUTED),
            );
            ui.add_space(6.0);
        }
        for (page, zh, en, glyph) in [
            (Page::Favorites, "收藏", "Favorites", Glyph::Heart),
            (Page::Recent, "最近播放", "Recent", Glyph::Clock),
            (Page::Downloads, "下载", "Downloads", Glyph::Download),
        ] {
            if nav_item(
                &mut ui,
                l.text(zh, en),
                glyph,
                self.workspace.page == page,
                narrow,
            )
            .clicked()
            {
                self.navigate(page);
            }
        }
        let settings = Rect::from_min_size(
            pos2(rect.min.x + 12.0, rect.max.y - 58.0),
            vec2(rect.width() - 24.0, 42.0),
        );
        let mut bottom = child(root, "nav-settings", settings);
        if nav_item(
            &mut bottom,
            l.text("设置", "Settings"),
            Glyph::Settings,
            self.workspace.page == Page::Settings,
            narrow,
        )
        .clicked()
        {
            self.navigate(Page::Settings);
        }
    }
}

pub(super) fn child(root: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, rect: Rect) -> Ui {
    let mut ui = root.new_child(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    ui.set_clip_rect(rect.intersect(root.clip_rect()));
    ui
}
pub(super) fn primary(ui: &mut Ui, label: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(BACKGROUND).strong())
            .fill(ACCENT)
            .stroke(Stroke::NONE),
    )
}
fn page_heading(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    ui.label(RichText::new(title).size(29.0).strong());
    if let Some(text) = subtitle {
        ui.label(RichText::new(text).color(MUTED));
    }
    ui.add_space(16.0);
}
fn nav_item(ui: &mut Ui, label: &str, icon: Glyph, selected: bool, narrow: bool) -> Response {
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 43.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if selected || response.hovered() {
        ui.painter()
            .rect_filled(r, 7, if selected { SELECTED } else { SURFACE });
    }
    if selected {
        ui.painter()
            .rect_filled(Rect::from_min_size(r.min, vec2(2.0, r.height())), 1, ACCENT);
    }
    let color = if selected { ACCENT } else { TEXT };
    glyph(
        ui.painter(),
        Rect::from_center_size(pos2(r.min.x + 24.0, r.center().y), vec2(20.0, 20.0)),
        color,
        icon,
    );
    if !narrow {
        ui.painter().text(
            pos2(r.min.x + 52.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(15.0),
            color,
        );
    }
    response.on_hover_text(label)
}
fn glyph(p: &Painter, r: Rect, c: Color32, g: Glyph) {
    let s = Stroke::new(1.6, c);
    match g {
        Glyph::Music => draw_music(p, r, c),
        Glyph::Video => draw_video(p, r, c),
        Glyph::Settings => draw_settings(p, r, c),
        Glyph::Book => {
            p.rect_stroke(r, 2, s, egui::StrokeKind::Inside);
            p.line_segment(
                [pos2(r.center().x, r.min.y), pos2(r.center().x, r.max.y)],
                s,
            );
        }
        Glyph::Home => {
            p.line_segment(
                [pos2(r.min.x, r.center().y), pos2(r.center().x, r.min.y)],
                s,
            );
            p.line_segment(
                [pos2(r.center().x, r.min.y), pos2(r.max.x, r.center().y)],
                s,
            );
            p.rect_stroke(
                Rect::from_min_max(pos2(r.min.x + 3.0, r.center().y), r.max - vec2(3.0, 0.0)),
                1,
                s,
                egui::StrokeKind::Inside,
            );
        }
        Glyph::Clock => {
            p.circle_stroke(r.center(), r.width() * 0.47, s);
            p.line_segment([r.center(), pos2(r.center().x, r.min.y + 4.0)], s);
            p.line_segment([r.center(), pos2(r.max.x - 4.0, r.center().y + 3.0)], s);
        }
        Glyph::Download => {
            p.line_segment(
                [
                    pos2(r.center().x, r.min.y),
                    pos2(r.center().x, r.max.y - 5.0),
                ],
                s,
            );
            p.line_segment(
                [
                    pos2(r.min.x + 3.0, r.center().y),
                    pos2(r.center().x, r.max.y - 5.0),
                ],
                s,
            );
            p.line_segment(
                [
                    pos2(r.max.x - 3.0, r.center().y),
                    pos2(r.center().x, r.max.y - 5.0),
                ],
                s,
            );
            p.line_segment([pos2(r.min.x, r.max.y), r.max], s);
        }
        Glyph::Heart => {
            let points = [
                (0.5, 0.95),
                (0.08, 0.50),
                (0.03, 0.27),
                (0.15, 0.08),
                (0.34, 0.06),
                (0.5, 0.23),
                (0.66, 0.06),
                (0.85, 0.08),
                (0.97, 0.27),
                (0.92, 0.50),
                (0.5, 0.95),
            ]
            .map(|(x, y)| pos2(r.min.x + r.width() * x, r.min.y + r.height() * y));
            p.add(Shape::line(points.to_vec(), s));
        }
    }
}
