use super::super::preferences::form_row;
use super::*;

impl App {
    pub(in crate::app) fn preferences_comics(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        self.preferences_komiic(ui);
        let settings = &mut self.preferences_draft.comics;
        form_row(
            ui,
            l.text("自动检查更新", "Check for updates"),
            l,
            |ui| {
                design::toggle(
                    ui,
                    &mut settings.check_updates,
                    l.text("自动检查更新", "Check for updates"),
                );
            },
        );
        form_row(ui, l.text("图像模型", "Image model"), l, |ui| {
            super::super::providers::model_selector(
                ui,
                &mut settings.image_selected,
                &self.preferences_draft.ai,
                l,
            );
        });
        form_row(ui, l.text("翻译语言", "Translation language"), l, |ui| {
            let selected_label = match settings.target.as_str() {
                "zh-CN" => "简体中文",
                "en-US" => "English",
                _ => l.text("跟随界面语言", "Follow interface"),
            };
            design::dropdown_ui(
                ui,
                "comic_target",
                ui.available_width(),
                selected_label,
                |ui| {
                    design::menu_value(
                        ui,
                        &mut settings.target,
                        "auto".into(),
                        l.text("跟随界面语言", "Follow interface"),
                    );
                    design::menu_value(ui, &mut settings.target, "zh-CN".into(), "简体中文");
                    design::menu_value(ui, &mut settings.target, "en-US".into(), "English");
                },
            );
        });
        form_row(ui, l.text("图像质量", "Image quality"), l, |ui| {
            let selected_label = if settings.image_quality.is_empty() {
                l.text("默认", "Default").to_owned()
            } else {
                settings.image_quality.clone()
            };
            design::dropdown_ui(
                ui,
                "comic_quality",
                ui.available_width(),
                &selected_label,
                |ui| {
                    for quality in ["", "low", "medium", "high"] {
                        design::menu_value(
                            ui,
                            &mut settings.image_quality,
                            quality.into(),
                            if quality.is_empty() {
                                l.text("默认", "Default")
                            } else {
                                quality
                            },
                        );
                    }
                },
            );
        });
        ui.add_space(4.0);
        if form_row(ui, "", l, |ui| {
            design::control(
                ui,
                l.text("清理缓存", "Clear cache"),
                Some(super::super::draw_trash),
                design::Control::Outline,
                0.0,
            )
        })
        .clicked()
        {
            let directory = domain::root().join("cache");
            match std::fs::remove_dir_all(&directory) {
                Ok(()) => {
                    images::clear();
                    self.comic.covers.clear();
                    self.comic.cover_used.clear();
                    self.comic.cover_jobs.clear();
                    self.comic.cover_errors.clear();
                    if let Some(reader) = &mut self.comic.reader {
                        reader.assets.clear();
                        reader.jobs.clear();
                    }
                    self.config_message = l
                        .text(
                            "已清理缓存，译图与阅读记录保留",
                            "Cache cleared; translations and reading records retained",
                        )
                        .into();
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => self.config_message = error.to_string(),
            }
        }
    }

    fn preferences_komiic(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        ui.label(RichText::new(l.text("Komiic 账号", "Komiic account")).strong());
        let account = &mut self.preferences_draft.comics.komiic;
        form_row(ui, l.text("邮箱", "Email"), l, |ui| {
            design::input_field(ui, &mut account.email, ui.available_width(), "");
        });
        form_row(ui, l.text("密码", "Password"), l, |ui| {
            design::secret_field(
                ui,
                "komiic-password",
                &mut account.password,
                ui.available_width(),
                "",
            );
        });
        form_row(ui, l.text("自动登录", "Automatic sign-in"), l, |ui| {
            design::toggle(
                ui,
                &mut account.auto_login,
                l.text("自动登录", "Automatic sign-in"),
            );
        });
        let busy = self.comic.auth_job.is_some();
        let (login, logout) = form_row(ui, "", l, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                let login = design::control(
                    ui,
                    l.text("保存并登录", "Save and sign in"),
                    None,
                    design::Control::Outline,
                    0.0,
                )
                .clicked();
                let logout = design::control(
                    ui,
                    l.text("退出并清除密码", "Sign out and clear password"),
                    None,
                    design::Control::Outline,
                    0.0,
                )
                .clicked();
                (login, logout)
            })
            .inner
        });
        if login || logout {
            let mut next = self.settings.clone();
            next.comics.komiic = self.preferences_draft.comics.komiic.clone();
            if logout {
                next.comics.komiic.auto_login = false;
                next.comics.komiic.password.clear();
            }
            let result = if login
                && (next.comics.komiic.email.trim().is_empty()
                    || next.comics.komiic.password.is_empty())
            {
                Err(anyhow::anyhow!(
                    "请填写 Komiic 邮箱和密码 / Enter Komiic credentials"
                ))
            } else {
                next.save()
            };
            match result {
                Err(error) => self.comic.auth_error = Some(error.to_string()),
                Ok(()) => {
                    self.settings = next;
                    self.preferences_draft.comics.komiic = self.settings.comics.komiic.clone();
                    self.comic.auth_configured = Some(self.settings.comics.komiic.clone());
                    self.comic.auth_error = None;
                    if let Some(source) = &self.comic.source {
                        let source = source.komiic().clone();
                        if login
                            && let Err(error) =
                                source.configure_account(self.settings.comics.komiic.clone())
                        {
                            self.comic.auth_error = Some(error.to_string());
                            return;
                        }
                        let ctx = ui.ctx().clone();
                        self.comic.auth_job = Some(Job::spawn(move |_| async move {
                            let result = if logout {
                                source.logout().await.map(|()| source.auth_status())
                            } else {
                                source.login().await
                            };
                            ctx.request_repaint();
                            result
                        }));
                    }
                }
            }
        }
        let status = self
            .comic
            .source
            .as_ref()
            .map(|source| source.komiic().auth_status());
        let message = self
            .comic
            .auth_error
            .as_deref()
            .or_else(|| status.as_ref().map(|s| s.label()))
            .unwrap_or("");
        ui.label(message);
        if let Some(status) = status {
            if !status.nickname.is_empty() {
                ui.label(status.nickname);
            }
            if let (Some(used), Some(limit)) = (status.usage, status.limit) {
                ui.label(format!(
                    "{}: {used}/{limit}",
                    l.text("图片用量", "Image usage")
                ));
            }
        }
        ui.label(l.text(
            "密码仅存本机凭据库，不参与云同步。",
            "Password stays in the local credential vault and is not cloud-synced.",
        ));
        ui.separator();
    }
}
