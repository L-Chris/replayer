use super::*;

impl App {
    pub(super) fn preferences(&mut self, _ctx: &egui::Context) {
        if self.settings_open {
            self.workspace.page = workspace::Page::Settings;
        }
    }
    pub(super) fn preferences_page(&mut self, ui: &mut Ui) {
        let language = self.settings.language;
        let dirty_before = self.preferences_draft != self.settings;
        ui.horizontal(|ui| {
            ui.heading(language.text("设置", "Settings"));
            if !matches!(
                self.preferences_tab,
                PrefTab::Providers | PrefTab::Cloud | PrefTab::About
            ) {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let response = design::icon_button(
                        ui,
                        icons::DOTS_THREE,
                        language.text("更多", "More"),
                        false,
                    );
                    egui::Popup::menu(&response)
                        .gap(4.0)
                        .width(148.0)
                        .show(|ui| {
                            if design::menu_item(
                                ui,
                                false,
                                language.text("恢复默认", "Restore defaults"),
                            )
                            .clicked()
                            {
                                self.reset_preferences(language);
                            }
                        });
                });
            }
        });
        ui.add_space(16.0);
        let rect = ui.available_rect_before_wrap();
        let nav_width = if rect.width() > 680.0 { 168.0 } else { 126.0 };
        let mut nav = workspace::child(
            ui,
            "settings-navigation",
            Rect::from_min_size(rect.min, vec2(nav_width, rect.height())),
        );
        nav.spacing_mut().item_spacing.y = 4.0;
        for (tab, zh, en, glyph) in [
            (
                PrefTab::General,
                "通用",
                "General",
                icons::SLIDERS_HORIZONTAL,
            ),
            (
                PrefTab::Providers,
                "AI 提供商",
                "AI providers",
                icons::SPARKLE,
            ),
            (
                PrefTab::Subtitles,
                "AI 字幕",
                "AI subtitles",
                icons::SUBTITLES,
            ),
            (
                PrefTab::MusicSources,
                "音乐相关",
                "Music",
                icons::MUSIC_NOTES,
            ),
            (PrefTab::Comics, "漫画相关", "Comics", icons::BOOK_OPEN),
            (
                PrefTab::Cloud,
                "云同步",
                "Cloud sync",
                icons::CLOUD_ARROW_UP,
            ),
            (PrefTab::About, "关于", "About", icons::INFO),
        ] {
            if design::nav_item(
                &mut nav,
                language.text(zh, en),
                Some(glyph),
                self.preferences_tab == tab,
                false,
            )
            .clicked()
                && self.preferences_tab != tab
            {
                self.preferences_tab = tab;
            }
        }
        let body = Rect::from_min_max(
            pos2(rect.min.x + nav_width + 24.0, rect.min.y),
            pos2(
                rect.max.x,
                rect.max.y - if dirty_before { 52.0 } else { 0.0 },
            ),
        );
        let mut content = workspace::child(ui, "settings-content", body);
        design::scroll()
            .id_salt(("preferences-page", self.preferences_tab as u8))
            .auto_shrink([false, false])
            .show(&mut content, |ui| {
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, design::LINE))
                    .corner_radius(12)
                    .inner_margin(24)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 14.0;
                        match self.preferences_tab {
                            PrefTab::General => self.preferences_general(ui),
                            PrefTab::Providers => self.preferences_providers(ui),
                            PrefTab::Subtitles => self.preferences_subtitles(ui),
                            PrefTab::MusicSources => self.preferences_music_sources(ui),
                            PrefTab::Comics => self.preferences_comics(ui),
                            PrefTab::Cloud => self.preferences_cloud(ui),
                            PrefTab::About => self.about(ui, &ui.ctx().clone()),
                        }
                    });
            });
        let dirty = self.preferences_draft != self.settings;
        if dirty != dirty_before {
            ui.ctx().request_repaint();
        }
        if dirty {
            let mut attempted_save = false;
            let mut footer = workspace::child(
                ui,
                "settings-footer",
                Rect::from_min_max(pos2(body.min.x, rect.max.y - 44.0), rect.max),
            );
            footer.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if workspace::primary(ui, language.text("保存", "Save")).clicked() {
                    self.save_preferences(language);
                    attempted_save = true;
                    ui.ctx().request_repaint();
                }
                if design::control(
                    ui,
                    language.text("放弃更改", "Discard"),
                    None,
                    design::Control::Outline,
                    0.0,
                )
                .clicked()
                {
                    self.preferences_draft = self.settings.clone();
                    ui.ctx().request_repaint();
                }
            });
            if attempted_save {
                ui.ctx()
                    .data_mut(|d| d.remove::<Feedback>(ui.id().with("settings-feedback-state")));
            }
        }
        self.preferences_feedback(ui, rect, dirty);
    }
    fn preferences_feedback(&self, ui: &Ui, rect: Rect, dirty: bool) {
        let id = ui.id().with("settings-feedback-state");
        if self.config_message.is_empty() {
            ui.ctx().data_mut(|d| d.remove::<Feedback>(id));
            return;
        }
        let notice = ui.ctx().data_mut(|data| {
            let previous = data.get_temp::<Feedback>(id);
            let notice = previous
                .filter(|n| n.message == self.config_message)
                .unwrap_or_else(|| Feedback {
                    message: self.config_message.clone(),
                    until: Instant::now() + Duration::from_secs(4),
                });
            data.insert_temp(id, notice.clone());
            notice
        });
        if let Some(remaining) = notice.until.checked_duration_since(Instant::now()) {
            egui::Area::new(ui.id().with("settings-feedback"))
                .order(egui::Order::Foreground)
                .interactable(false)
                .pivot(egui::Align2::RIGHT_BOTTOM)
                .fixed_pos(rect.max - vec2(0.0, if dirty { 54.0 } else { 0.0 }))
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(SURFACE)
                        .corner_radius(8)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.label(&notice.message);
                        });
                });
            ui.ctx().request_repaint_after(remaining);
        }
    }
    fn save_preferences(&mut self, language: Language) {
        let result = self
            .preferences_draft
            .ai
            .validate()
            .and_then(|_| subtitles::LlmConfig::validate_options(&self.preferences_draft.llm))
            .and_then(|_| self.preferences_draft.save());
        match result {
            Ok(()) => {
                self.settings = self.preferences_draft.clone();
                self.online_music.clear_search();
                self.comic.reload_images();
                self.config_message = language.text("已保存", "Saved").into();
            }
            Err(error) => self.config_message = language.error(&error.to_string()),
        }
    }
    fn reset_preferences(&mut self, language: Language) {
        match self.preferences_tab {
            PrefTab::General => self.preferences_draft.language = Language::Chinese,
            PrefTab::Subtitles => {
                let selected = self.preferences_draft.llm.selected.clone();
                self.preferences_draft.llm = crate::settings::LlmSettings {
                    selected,
                    ..Default::default()
                };
            }
            PrefTab::Comics => {
                let image_selected = self.preferences_draft.comics.image_selected.clone();
                self.preferences_draft.comics = crate::comics::Settings {
                    image_selected,
                    ..Default::default()
                };
            }
            PrefTab::MusicSources => {
                let api_key = self.preferences_draft.music.api_key.clone();
                self.preferences_draft.music = crate::music_sources::Settings {
                    api_key,
                    ..Default::default()
                };
            }
            PrefTab::Providers | PrefTab::Cloud | PrefTab::About => return,
        }
        self.config_message = language.text("已恢复默认值", "Defaults restored").into();
    }
    fn preferences_general(&mut self, ui: &mut Ui) {
        let language = self.settings.language;
        form_row(
            ui,
            language.text("界面语言", "Interface language"),
            language,
            |ui| {
                let mut idx = match self.preferences_draft.language {
                    Language::Chinese => 0,
                    Language::English => 1,
                };
                if design::segmented(ui, &mut idx, &["简体中文", "English"]) {
                    self.preferences_draft.language = if idx == 0 {
                        Language::Chinese
                    } else {
                        Language::English
                    };
                }
            },
        );
    }

    fn preferences_subtitles(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        let settings = &mut self.preferences_draft.llm;
        form_row(ui, l.text("字幕模型", "Subtitle model"), l, |ui| {
            providers::model_selector(ui, &mut settings.selected, &self.preferences_draft.ai, l);
        });
        let mut bilingual = settings.bilingual.unwrap_or(false);
        form_row(ui, l.text("双语字幕", "Bilingual subtitles"), l, |ui| {
            if design::toggle(
                ui,
                &mut bilingual,
                l.text("双语字幕", "Bilingual subtitles"),
            )
            .changed()
            {
                settings.bilingual = Some(bilingual);
            }
        });
        form_row(ui, l.text("音频分段", "Chunk duration"), l, |ui| {
            design::unit_field(
                ui,
                &mut settings.chunk_seconds,
                128.0,
                "120",
                l.text("秒", "s"),
            );
        });
        form_row(ui, l.text("并发请求", "Parallel requests"), l, |ui| {
            design::stepper_usize(ui, &mut self.preferences_draft.subtitle_concurrency, 1..=6);
        });
        form_row(ui, l.text("思考等级", "Reasoning effort"), l, |ui| {
            let selected_label =
                if settings.reasoning_effort.is_empty() || settings.reasoning_effort == "default" {
                    l.text("默认", "Default").to_owned()
                } else {
                    settings.reasoning_effort.clone()
                };
            design::dropdown_ui(
                ui,
                "reasoning_effort",
                ui.available_width(),
                &selected_label,
                |ui| {
                    for effort in [
                        "default", "none", "minimal", "low", "medium", "high", "xhigh", "max",
                    ] {
                        design::menu_value(
                            ui,
                            &mut settings.reasoning_effort,
                            effort.into(),
                            if effort == "default" {
                                l.text("默认", "Default")
                            } else {
                                effort
                            },
                        );
                    }
                },
            );
        });
        form_row(ui, l.text("音频格式", "Audio format"), l, |ui| {
            let selected_label = settings.audio_format.to_uppercase();
            design::dropdown_ui(
                ui,
                "audio_format",
                ui.available_width(),
                &selected_label,
                |ui| {
                    for format in ["mp3", "wav"] {
                        let upper = format.to_uppercase();
                        design::menu_value(ui, &mut settings.audio_format, format.into(), &upper);
                    }
                },
            );
        });
        form_row(ui, l.text("音频编码", "Audio encoding"), l, |ui| {
            let selected_label = if settings.audio_encoding == "auto" {
                l.text("自动", "Automatic").to_owned()
            } else {
                settings.audio_encoding.clone()
            };
            design::dropdown_ui(
                ui,
                "audio_encoding",
                ui.available_width(),
                &selected_label,
                |ui| {
                    for encoding in ["auto", "data_url", "base64"] {
                        design::menu_value(
                            ui,
                            &mut settings.audio_encoding,
                            encoding.into(),
                            if encoding == "auto" {
                                l.text("自动", "Automatic")
                            } else {
                                encoding
                            },
                        );
                    }
                },
            );
        });
    }
}

/// Settings use a shared label column and a bounded control column, including
/// rows with checkboxes or auxiliary actions that have no label of their own.
pub(super) fn form_row<R>(
    ui: &mut Ui,
    label: &str,
    language: Language,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let label_width = match language {
        Language::Chinese => 96.0,
        Language::English => 140.0,
    };
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(label_width, design::CONTROL_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(label_width, design::CONTROL_HEIGHT));
                ui.add(egui::Label::new(RichText::new(label).color(MUTED)).wrap());
            },
        );
        let width = ui.available_width().min(360.0);
        ui.allocate_ui_with_layout(
            vec2(width, design::CONTROL_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_height(design::CONTROL_HEIGHT);
                contents(ui)
            },
        )
        .inner
    })
    .inner
}

#[derive(Clone)]
struct Feedback {
    message: String,
    until: Instant,
}
