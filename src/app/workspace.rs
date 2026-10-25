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
}

#[derive(Clone, Copy)]
enum Glyph {
    Home,
    Music,
    Video,
    Book,
    Settings,
}

impl App {
    pub(super) fn navigate(&mut self, page: Page) {
        if page == Page::Home && self.workspace.page != Page::Home {
            self.comic.refresh_on_home(&self.settings, Instant::now());
        }
        self.update_library();
        self.library.flush();
        self.comic.flush();
        if matches!(page, Page::Comic | Page::Home | Page::Music | Page::Video)
            && !self.is_music()
            && let Some(player) = &self.player
        {
            player.set_playing(false);
        }
        if page == Page::Settings && !self.settings_open {
            self.preferences_draft = self.settings.clone();
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
            if window::fullscreen(&ctx) {
                self.fullscreen = false;
                window::set_fullscreen(&ctx, false);
            } else if self.workspace.page == Page::Settings && egui::Popup::is_any_open(&ctx) {
                egui::Popup::close_all(&ctx);
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
            && !self.updater.dialog_visible
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
                        (content.width() - 56.0 - window::controls_width(&ctx)).max(180.0),
                        36.0,
                    ),
                ),
            );
            header.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                if design::back_button(ui, language.text("返回音乐库", "Back to music library"))
                    .clicked()
                {
                    self.navigate(Page::Music);
                }
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
                    pos2(
                        content.max.x - width - 24.0,
                        content.min.y + music::TABS_TOP,
                    ),
                    vec2(width, design::CONTROL_HEIGHT),
                ),
            );
            design::segmented(&mut tabs, &mut self.workspace.listening_tab, &labels);
            self.music_view(root, screen, content, &mut act);
        } else {
            let settings_page = self.workspace.page == Page::Settings;
            if !settings_page {
                let mut top = child(
                    root,
                    "workspace-header",
                    Rect::from_min_size(
                        content.min + vec2(28.0, 12.0),
                        vec2(
                            (content
                                .max
                                .x
                                .min(screen.max.x - window::controls_width(&ctx) - 12.0)
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
                        (screen.max.x - window::controls_width(&ctx) - 8.0).min(content.max.x),
                        content.min.y + 60.0,
                    ),
                ));
                top.horizontal(|ui| {
                    let width = ui.available_width().clamp(100.0, 530.0);
                    let input = design::search_field(
                        ui,
                        &mut self.workspace.query,
                        width,
                        language.text("搜索媒体、音乐或漫画", "Search your media"),
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
            }
            let body = Rect::from_min_max(
                content.min + vec2(28.0, if settings_page { 52.0 } else { 84.0 }),
                content.max - vec2(28.0, 14.0),
            );
            let mut ui = child(root, "workspace-content", body);
            match self.workspace.page {
                Page::Home => self.home_page(&mut ui),
                Page::Comic => {
                    if !self.comic.detail_open() {
                        ui.label(
                            RichText::new(language.text("漫画", "Comics"))
                                .size(29.0)
                                .strong(),
                        );
                        ui.add_space(4.0);
                    }
                    self.comic
                        .ui(&mut ui, &mut self.settings, &mut self.settings_open);
                }
                Page::Settings => self.preferences_page(&mut ui),
                Page::Music | Page::Video => self.media_library(&mut ui),
                _ => {}
            }
            if matches!(self.workspace.page, Page::Home | Page::Music | Page::Video) {
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
        self.history_notice(&ctx);
        self.update_dialog(&ctx);
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
            branding::paint(ui.painter(), r);
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
    design::control(ui, label, None, design::Control::Primary, 0.0)
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
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label)
}
fn glyph(p: &Painter, r: Rect, c: Color32, g: Glyph) {
    let icon = match g {
        Glyph::Home => icons::HOUSE,
        Glyph::Music => icons::MUSIC_NOTES,
        Glyph::Video => icons::FILM_SLATE,
        Glyph::Book => icons::BOOK_OPEN,
        Glyph::Settings => icons::GEAR_SIX,
    };
    icons::paint(p, r, c, icon);
}
