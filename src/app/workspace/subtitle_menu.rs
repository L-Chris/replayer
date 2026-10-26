use super::*;

impl App {
    pub(super) fn subtitle_menu(&mut self, ui: &mut Ui, a: f32, act: &mut Act) {
        let language = self.settings.language;
        let generating = self.subtitle_job.is_some();
        let ready = !self.subtitles.is_empty();
        let caption = if generating {
            language.text("AI 字幕 · 生成中", "AI subtitles · generating")
        } else if ready {
            language.text("AI 字幕 · 就绪", "AI subtitles · ready")
        } else {
            language.text("字幕", "Subtitles")
        };
        let active = generating || (ready && self.subtitles_visible);
        let subtitles = design::icon_button_sized(ui, 28.0, a, active, caption, draw_subtitles);
        if generating || ready {
            ui.painter().circle_filled(
                subtitles.rect.right_top() + vec2(-5.0, 5.0),
                3.0,
                ACCENT.gamma_multiply(a),
            );
        }
        egui::Popup::menu(&subtitles).show(|ui| {
            ui.set_min_width(200.0);
            ui.set_max_width(280.0);
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
                        design::scroll().max_height(160.0).show(ui, |ui| {
                            for (start, end, error) in &self.subtitle_skipped {
                                ui.label(format!("{}–{}", fmt_time(*start), fmt_time(*end)));
                                ui.small(language.error(error));
                            }
                        });
                    },
                );
                ui.separator();
            }
            ui.horizontal(|ui| {
                ui.label(language.text("显示字幕", "Show subtitles"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    design::toggle(
                        ui,
                        &mut self.subtitles_visible,
                        language.text("显示字幕", "Show subtitles"),
                    );
                });
            });
            ui.separator();
            if generating {
                if design::menu_item(ui, false, language.text("取消生成", "Cancel generation"))
                    .clicked()
                {
                    *act = Act::CancelSubtitles;
                    ui.close();
                }
            } else if design::menu_item(
                ui,
                false,
                if ready {
                    language.text("重新生成字幕", "Regenerate subtitles")
                } else {
                    language.text("生成 AI 字幕", "Generate AI subtitles")
                },
            )
            .clicked()
            {
                *act = Act::GenerateSubtitles;
                ui.close();
            }
            ui.add_enabled_ui(ready, |ui| {
                if design::menu_item(ui, false, language.text("导出 SRT…", "Export SRT…"))
                    .clicked()
                {
                    *act = Act::ExportSubtitles;
                    ui.close();
                }
            });
        });
    }
}
