use super::*;

impl App {
    pub(in crate::app) fn preferences_comics(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        let settings = &mut self.settings.comics;
        ui.heading(l.text("漫画阅读与翻译", "Comics and translation"));
        ui.checkbox(
            &mut settings.check_updates,
            l.text(
                "每 6 小时检查收藏更新（应用运行时）",
                "Check favorites every 6 hours while running",
            ),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(l.text("图像编辑服务", "Image editing service")).strong());
        ui.label(l.text(
            "地址留空继承 IMAGE_BASE_URL / LLM_BASE_URL",
            "Empty endpoint: IMAGE_BASE_URL / LLM_BASE_URL",
        ));
        ui.add(
            egui::TextEdit::singleline(&mut settings.image_base_url)
                .hint_text("https://api.example.com/v1")
                .desired_width(f32::INFINITY),
        );
        ui.label(l.text(
            "图像编辑模型（IMAGE_MODEL）",
            "Image editing model (IMAGE_MODEL)",
        ));
        ui.add(
            egui::TextEdit::singleline(&mut settings.image_model)
                .hint_text(l.text("留空使用 .env", "Empty: use .env")),
        );
        ui.label("API Key");
        ui.add(
            egui::TextEdit::singleline(&mut settings.image_key)
                .password(true)
                .hint_text("IMAGE_API_KEY / LLM_API_KEY"),
        );
        ui.small(l.text(
            "设置中的密钥仅本次运行有效；永久配置请使用 .env",
            "Settings keys are session-only; use .env to persist",
        ));
        ui.horizontal(|ui| {
            ui.label(l.text("翻译语言", "Translation language"));
            egui::ComboBox::from_id_salt("comic_target")
                .selected_text(match settings.target.as_str() {
                    "zh-CN" => "简体中文",
                    "en-US" => "English",
                    _ => l.text("跟随界面语言", "Follow interface"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.target,
                        "auto".into(),
                        l.text("跟随界面语言", "Follow interface"),
                    );
                    ui.selectable_value(&mut settings.target, "zh-CN".into(), "简体中文");
                    ui.selectable_value(&mut settings.target, "en-US".into(), "English");
                });
        });
        ui.horizontal(|ui| {
            ui.label(l.text("图像质量", "Image quality"));
            egui::ComboBox::from_id_salt("comic_quality")
                .selected_text(if settings.image_quality.is_empty() {
                    "Default"
                } else {
                    &settings.image_quality
                })
                .show_ui(ui, |ui| {
                    for quality in ["", "low", "medium", "high"] {
                        ui.selectable_value(
                            &mut settings.image_quality,
                            quality.into(),
                            if quality.is_empty() {
                                "Default"
                            } else {
                                quality
                            },
                        );
                    }
                });
        });
        ui.small(l.text("默认不传 quality；其他级别需模型支持。每次图像编辑最长 180 秒，不自动重试。","Default omits quality. Other levels require model support. Each edit has a 180-second timeout and no automatic retry."));
        ui.add_space(8.0);
        ui.label(RichText::new(l.text("本地保存", "Local storage")).strong());
        ui.small(domain::root().display().to_string());
        ui.label(l.text("书架、阅读位置和更新状态永久保存；原图与译图分别缓存。清理图片不影响收藏和历史。","Library, progress and updates persist. Originals and translations are cached separately; clearing images keeps your library and history."));
        if ui
            .button(l.text("清理原图和封面缓存", "Clear originals and covers"))
            .clicked()
        {
            let directory = domain::root().join("cache");
            match std::fs::remove_dir_all(&directory) {
                Ok(()) => {
                    self.comic.covers.clear();
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
}
