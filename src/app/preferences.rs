use super::*;

impl App {
    pub(super) fn preferences(&mut self, _ctx: &egui::Context) {
        if self.settings_open {
            self.workspace.page = workspace::Page::Settings;
        }
    }
    pub(super) fn preferences_page(&mut self, ui: &mut Ui) {
        let language = self.settings.language;
        ui.heading(language.text("设置", "Settings"));
        ui.label(
            RichText::new(language.text(
                "管理播放体验与 AI 服务",
                "Playback experience and AI services",
            ))
            .color(MUTED),
        );
        ui.add_space(20.0);
        let rect = ui.available_rect_before_wrap();
        let nav_width = if rect.width() > 680.0 { 155.0 } else { 110.0 };
        let mut nav = workspace::child(
            ui,
            "settings-navigation",
            Rect::from_min_size(rect.min, vec2(nav_width, rect.height())),
        );
        nav.spacing_mut().item_spacing.y = 10.0;
        for (tab, zh, en) in [
            (PrefTab::General, "外观与语言", "Appearance"),
            (PrefTab::Subtitles, "AI 字幕", "AI subtitles"),
            (PrefTab::MusicSources, "音乐源", "Music sources"),
            (PrefTab::Comics, "漫画与图片翻译", "Comics & translation"),
            (PrefTab::About, "关于", "About"),
        ] {
            nav.selectable_value(&mut self.preferences_tab, tab, language.text(zh, en));
        }
        let body = Rect::from_min_max(
            pos2(rect.min.x + nav_width + 22.0, rect.min.y),
            pos2(rect.max.x, rect.max.y - 68.0),
        );
        let mut content = workspace::child(ui, "settings-content", body);
        egui::ScrollArea::vertical()
            .id_salt(("preferences-page", self.preferences_tab as u8))
            .auto_shrink([false, false])
            .show(&mut content, |ui| {
                egui::Frame::new()
                    .fill(SURFACE)
                    .corner_radius(10)
                    .inner_margin(20)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match self.preferences_tab {
                            PrefTab::General => self.preferences_general(ui),
                            PrefTab::Subtitles => self.preferences_subtitles(ui),
                            PrefTab::MusicSources => self.preferences_music_sources(ui),
                            PrefTab::Comics => self.preferences_comics(ui),
                            PrefTab::About => self.about(ui, &ui.ctx().clone()),
                        }
                    });
            });
        let mut footer = workspace::child(
            ui,
            "settings-footer",
            Rect::from_min_max(pos2(body.min.x, rect.max.y - 57.0), rect.max),
        );
        footer.horizontal(|ui| {
            if ui
                .button(language.text("恢复默认", "Restore defaults"))
                .clicked()
            {
                self.reset_preferences(language);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if workspace::primary(ui, language.text("保存设置", "Save settings")).clicked()
                {
                    self.save_preferences(language);
                }
            });
        });
        if !self.config_message.is_empty() {
            footer.small(&self.config_message);
        }
    }
    fn save_preferences(&mut self, language: Language) {
        if self.preferences_tab == PrefTab::Comics {
            self.config_message = match self.settings.save() {
                Ok(()) => {
                    self.comic.reload_images();
                    language.text("已保存", "Saved").into()
                }
                Err(error) => error.to_string(),
            };
            return;
        }
        if self.preferences_tab != PrefTab::Subtitles {
            let previous = self.settings.music.clone();
            self.settings.music = self.online_music.draft.clone();
            self.config_message = match self.settings.save() {
                Ok(()) => {
                    if previous.source != self.settings.music.source {
                        self.online_music.clear_search();
                    }
                    language.text("已保存", "Saved").into()
                }
                Err(error) => {
                    self.settings.music = previous;
                    error.to_string()
                }
            };
            return;
        }
        match subtitles::LlmConfig::load_with(&self.llm_draft) {
            Ok(_) => {
                let previous = self.settings.llm.clone();
                self.settings.llm = self.llm_draft.clone();
                match self.settings.save() {
                    Ok(()) => {
                        self.config_message = language
                            .text(
                                "已保存 · 下次生成时生效",
                                "Saved · Applies to the next generation",
                            )
                            .into()
                    }
                    Err(error) => {
                        self.settings.llm = previous;
                        self.config_message = error.to_string();
                    }
                }
            }
            Err(error) => self.config_message = language.error(&error.to_string()),
        }
    }
    fn reset_preferences(&mut self, language: Language) {
        if self.preferences_tab == PrefTab::Comics {
            self.settings.comics = Default::default();
            self.config_message = language
                .text(
                    "已恢复默认值，点击保存应用",
                    "Defaults restored; save to apply",
                )
                .into();
            return;
        }
        if self.preferences_tab == PrefTab::MusicSources {
            self.online_music.draft = Default::default();
            self.config_message = language
                .text(
                    "已恢复默认值，点击保存应用",
                    "Defaults restored; save to apply",
                )
                .into();
            return;
        }
        self.llm_draft = Default::default();
        match subtitles::LlmConfig::defaults() {
            Ok(mut defaults) => {
                self.env_key_available = !defaults.api_key.is_empty();
                defaults.api_key.clear();
                self.llm_defaults = defaults;
                self.config_message = language
                    .text(
                        "已恢复默认值，点击保存应用",
                        "Defaults restored; save to apply",
                    )
                    .into();
            }
            Err(error) => self.config_message = language.error(&error.to_string()),
        }
    }
    fn preferences_general(&mut self, ui: &mut Ui) {
        let language = self.settings.language;
        ui.add_space(6.0);
        ui.label(
            RichText::new(language.text("界面语言", "Interface language"))
                .strong()
                .size(16.0),
        );
        ui.label(
            RichText::new(language.text(
                "语言同时用于界面与 AI 字幕",
                "Language applies to the interface and AI subtitles",
            ))
            .color(MUTED),
        );
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.settings.language, Language::Chinese, "简体中文");
            ui.selectable_value(&mut self.settings.language, Language::English, "English");
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(
            RichText::new(language.text("播放队列", "Play queue"))
                .strong()
                .size(16.0),
        );
        ui.label(
            RichText::new(language.text(
                "队列自动保存，下次启动时恢复",
                "The queue is saved automatically and restored on startup",
            ))
            .color(MUTED),
        );
    }
    fn preferences_subtitles(&mut self, ui: &mut Ui) {
        let language = self.settings.language;
        ui.add_space(6.0);
        ui.label(
            RichText::new(language.text("AI 字幕", "AI subtitles"))
                .strong()
                .size(16.0),
        );
        let mut bilingual = self
            .llm_draft
            .bilingual
            .or(self.llm_defaults.bilingual)
            .unwrap_or(false);
        if ui
            .checkbox(
                &mut bilingual,
                language.text(
                    "双语字幕（原文 + 目标语言）",
                    "Bilingual subtitles (original + target)",
                ),
            )
            .changed()
        {
            self.llm_draft.bilingual = Some(bilingual);
        }
        ui.label(
            RichText::new(language.text(
                "使用兼容 OpenAI 的音频模型 · 留空继承 .env",
                "OpenAI-compatible audio models · Empty fields inherit .env",
            ))
            .color(MUTED),
        );
        field(
            ui,
            language.text("接口地址", "Base URL"),
            &mut self.llm_draft.base_url,
            &self.llm_defaults.base_url,
            false,
        );
        field(
            ui,
            language.text("模型", "Model"),
            &mut self.llm_draft.model,
            &self.llm_defaults.model,
            false,
        );
        let key_hint = if self.env_key_available {
            language.text(
                "已从环境配置读取 · 留空继续使用",
                "Available from environment · Leave empty to inherit",
            )
        } else {
            language.text("输入 API Key", "Enter API key")
        };
        field(ui, "API Key", &mut self.llm_draft.api_key, key_hint, true);
        ui.small(
            RichText::new(language.text(
                "密钥仅本次运行有效；长期使用请配置 .env",
                "Keys stay in this session; use .env for persistence",
            ))
            .color(MUTED),
        );
        ui.add_space(4.0);
        ui.collapsing(language.text("高级选项", "Advanced"), |ui| {
            let effort = if self.llm_draft.reasoning_effort.is_empty() {
                self.llm_defaults.reasoning_effort.as_str()
            } else {
                self.llm_draft.reasoning_effort.as_str()
            };
            egui::ComboBox::from_label(language.text("思考等级", "Reasoning effort"))
                .selected_text(if effort.is_empty() || effort == "default" {
                    language.text("default（不传参数）", "default (omit parameter)")
                } else {
                    effort
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.llm_draft.reasoning_effort,
                        String::new(),
                        language.text("跟随 .env", "Use .env"),
                    );
                    for effort in [
                        "default", "none", "minimal", "low", "medium", "high", "xhigh", "max",
                    ] {
                        ui.selectable_value(
                            &mut self.llm_draft.reasoning_effort,
                            effort.to_owned(),
                            effort,
                        );
                    }
                });
            ui.small(language.text(
                "default 不传参数；其他等级需模型支持",
                "default omits the parameter; other levels require model support",
            ));
            egui::ComboBox::from_label(language.text("上传音频格式", "Upload audio format"))
                .selected_text(if self.llm_draft.audio_format.is_empty() {
                    language.text("跟随 .env（默认 MP3）", "Use .env (MP3 by default)")
                } else {
                    &self.llm_draft.audio_format
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.llm_draft.audio_format,
                        String::new(),
                        language.text("跟随 .env", "Use .env"),
                    );
                    ui.selectable_value(&mut self.llm_draft.audio_format, "mp3".into(), "MP3");
                    ui.selectable_value(&mut self.llm_draft.audio_format, "wav".into(), "WAV");
                });
            egui::ComboBox::from_label(language.text("音频编码", "Audio encoding"))
                .selected_text(if self.llm_draft.audio_encoding.is_empty() {
                    language.text("跟随 .env", "Use .env")
                } else {
                    &self.llm_draft.audio_encoding
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.llm_draft.audio_encoding,
                        String::new(),
                        language.text("跟随 .env", "Use .env"),
                    );
                    for value in ["auto", "data_url", "base64"] {
                        ui.selectable_value(
                            &mut self.llm_draft.audio_encoding,
                            value.to_owned(),
                            value,
                        );
                    }
                });
            field(
                ui,
                language.text("音频分段（5–120 秒）", "Chunk duration (5–120 seconds)"),
                &mut self.llm_draft.chunk_seconds,
                &self.llm_defaults.chunk_seconds,
                false,
            );
        });
        ui.horizontal(|ui| {
            ui.label(language.text("并发请求", "Parallel requests"));
            let fraction = (self.settings.subtitle_concurrency.clamp(1, 6) - 1) as f32 / 5.0;
            let (track, response) = hslider(ui, 120.0, 34.0, fraction, 3.0, false, 1.0);
            if (response.clicked() || response.dragged())
                && let Some(p) = response.interact_pointer_pos()
            {
                self.settings.subtitle_concurrency =
                    (((p.x - track.min.x) / track.width()).clamp(0.0, 1.0) * 5.0).round() as usize
                        + 1;
            }
            ui.add(
                egui::DragValue::new(&mut self.settings.subtitle_concurrency)
                    .range(1..=6)
                    .speed(1.0),
            );
        });
        ui.small(
            RichText::new(language.text(
                "模型配置与并发数用于下一次生成",
                "Model settings and concurrency apply to the next generation",
            ))
            .color(MUTED),
        );
    }
}

fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str, password: bool) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(label)
            .hint_text(hint)
            .password(password)
            .desired_width(f32::INFINITY)
            .margin(vec2(10.0, 9.0)),
    );
}
