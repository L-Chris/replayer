use super::super::preferences::form_row;
use super::*;

impl App {
    pub(in crate::app) fn preferences_comics(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
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
            )
            .on_hover_text(l.text(
                "请选择支持编辑并返回图片的模型",
                "Choose a model that edits and returns images",
            ));
        });
        form_row(ui, l.text("翻译语言", "Translation language"), l, |ui| {
            egui::ComboBox::from_id_salt("comic_target")
                .width(ui.available_width())
                .selected_text(match settings.target.as_str() {
                    "zh-CN" => "简体中文",
                    "en-US" => "English",
                    _ => l.text("跟随界面语言", "Follow interface"),
                })
                .show_ui(ui, |ui| {
                    design::selectable_value(
                        ui,
                        &mut settings.target,
                        "auto".into(),
                        l.text("跟随界面语言", "Follow interface"),
                    );
                    design::selectable_value(ui, &mut settings.target, "zh-CN".into(), "简体中文");
                    design::selectable_value(ui, &mut settings.target, "en-US".into(), "English");
                });
        });
        form_row(ui, l.text("图像质量", "Image quality"), l, |ui| {
            egui::ComboBox::from_id_salt("comic_quality")
                .width(ui.available_width())
                .selected_text(if settings.image_quality.is_empty() {
                    l.text("默认", "Default")
                } else {
                    &settings.image_quality
                })
                .show_ui(ui, |ui| {
                    for quality in ["", "low", "medium", "high"] {
                        design::selectable_value(
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
                });
        });
        ui.add_space(8.0);
        if form_row(ui, "", l, |ui| ui.button(l.text("清理缓存", "Clear cache"))).clicked() {
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
}
