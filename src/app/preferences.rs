use super::*;

impl App {
    pub(super) fn preferences(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            self.preferences_armed = false;
            return;
        }
        let pressed_outside = self.preferences_armed
            && ctx.input(|i| i.pointer.any_pressed())
            && ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pointer| {
                self.preferences_rect
                    .is_some_and(|rect| !rect.contains(pointer))
            });
        if pressed_outside {
            self.settings_open = false;
            self.preferences_armed = false;
            return;
        }
        self.preferences_armed = true;
        let language = self.settings.language;
        let mut open = self.settings_open;
        let mut closed = false;
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() - 64.0).clamp(320.0, 620.0),
            (screen.height() - 160.0).clamp(260.0, 520.0),
        );
        let shown = egui::Window::new(language.text("设置", "Settings"))
            .id(Id::new("preferences"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(egui::Frame::window(&ctx.style_of(egui::Theme::Dark)).inner_margin(20))
            .fixed_size(size)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(language.text("设置", "Settings"))
                            .size(14.0)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, 20.0, 1.0, draw_close).clicked() {
                            closed = true;
                        }
                    });
                });
                ui.separator();
                let content_height = (size.y - 150.0).max(120.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|nav| {
                        nav.set_width(88.0);
                        nav.spacing_mut().item_spacing.y = 4.0;
                        nav.selectable_value(
                            &mut self.preferences_tab,
                            PrefTab::General,
                            language.text("通用", "General"),
                        );
                        nav.selectable_value(
                            &mut self.preferences_tab,
                            PrefTab::Subtitles,
                            language.text("字幕", "Subtitles"),
                        );
                        nav.selectable_value(
                            &mut self.preferences_tab,
                            PrefTab::About,
                            language.text("关于", "About"),
                        );
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(content_height)
                        .show(ui, |ui| {
                            // The scroll content inherits the row's horizontal
                            // layout; force the section back to a vertical stack.
                            ui.vertical(|ui| match self.preferences_tab {
                                PrefTab::General => self.preferences_general(ui),
                                PrefTab::Subtitles => self.preferences_subtitles(ui),
                                PrefTab::About => self.about(ui, ctx),
                            });
                        });
                });
                ui.add_space(10.0);
                if !self.config_message.is_empty() {
                    ui.label(&self.config_message);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(language.text("保存", "Save")).fill(ACCENT))
                            .clicked()
                        {
                            self.save_preferences(language);
                        }
                        if ui.button(language.text("重置", "Reset")).clicked() {
                            self.reset_preferences(language);
                        }
                    });
                });
                ui.add_space(2.0);
            });
        if let Some(inner) = shown {
            self.preferences_rect = Some(inner.response.rect);
        }
        self.settings_open = open && !closed;
    }
    fn save_preferences(&mut self, language: Language) {
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
        ui.add(
            egui::Slider::new(&mut self.settings.subtitle_concurrency, 1..=6)
                .text(language.text("并发请求", "Parallel requests")),
        );
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
