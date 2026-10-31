use super::super::preferences::form_row;
use super::*;

impl App {
    pub(in crate::app) fn preferences_comics(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        self.preferences_comic_service(ui);
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

    fn preferences_comic_service(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        ui.label(RichText::new(l.text("内容服务", "Content service")).strong());
        let service = &mut self.preferences_draft.comics.service;
        form_row(ui, l.text("服务地址", "Service URL"), l, |ui| {
            design::input_field(
                ui,
                &mut service.url,
                ui.available_width(),
                "https://content.example.com",
            );
        });
        form_row(ui, l.text("设备 Token", "Device token"), l, |ui| {
            design::secret_field(
                ui,
                "content-service-token",
                &mut service.token,
                ui.available_width(),
                "pt_…",
            );
        });
        let busy = self.comic.site_job.is_some();
        if form_row(ui, "", l, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                design::control(
                    ui,
                    l.text("保存并测试连接", "Save and test connection"),
                    None,
                    design::Control::Outline,
                    0.0,
                )
            })
            .inner
        })
        .clicked()
        {
            let mut next = self.settings.clone();
            next.comics.service = self.preferences_draft.comics.service.clone();
            next.comics.service.url = next.comics.service.url.trim().trim_end_matches('/').into();
            match next.save() {
                Ok(()) => {
                    self.settings = next;
                    self.preferences_draft.comics.service = self.settings.comics.service.clone();
                    self.comic.service_configured = None;
                    self.comic.service_error = None;
                }
                Err(error) => self.comic.service_error = Some(error.to_string()),
            }
        }
        if busy {
            ui.label(l.text("正在连接…", "Connecting…"));
        } else if let Some(error) = &self.comic.service_error {
            ui.label(error);
        } else if self.comic.source.is_some() {
            ui.label(l.text("连接正常", "Connected"));
            for site in &self.comic.sites {
                ui.label(&site.name);
            }
        }
        if !self.settings.comics.service.url.is_empty() {
            ui.hyperlink_to(
                l.text("打开管理页面", "Open management page"),
                &self.settings.comics.service.url,
            );
        }
        ui.label(l.text("源账号请在服务管理页面配置。设备 Token 仅存本机凭据库，不参与云同步。",
            "Manage source accounts on the service. The device token stays in this device's credential vault."));
        ui.separator();
    }
}
