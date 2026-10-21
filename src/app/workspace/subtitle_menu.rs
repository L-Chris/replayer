use super::*;

impl App {
    pub(super) fn subtitle_menu(&mut self, ui: &mut Ui, a: f32, act: &mut Act) {
        let language = self.settings.language;
        let caption = if self.subtitle_job.is_some() {
            language.text("字幕 · 生成中", "Subtitles · generating")
        } else {
            language.text("字幕", "Subtitles")
        };
        let subtitles = icon_button(ui, 26.0, a, draw_subtitles);
        let subtitles = subtitles.on_hover_text(caption);
        egui::Popup::menu(&subtitles).show(|ui| {
                ui.set_max_width(300.0);
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
                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for (start, end, error) in &self.subtitle_skipped {
                                        ui.label(format!(
                                            "{}–{}",
                                            fmt_time(*start),
                                            fmt_time(*end)
                                        ));
                                        ui.small(language.error(error));
                                    }
                                });
                        },
                    );
                }
                ui.checkbox(
                    &mut self.subtitles_visible,
                    language.text("显示字幕", "Show subtitles"),
                );
                if self.subtitle_job.is_some() {
                    if ui
                        .button(language.text("取消生成", "Cancel generation"))
                        .clicked()
                    {
                        *act = Act::CancelSubtitles;
                        ui.close();
                    }
                } else if ui
                    .add(egui::Button::new(if self.subtitles.is_empty() {
                        language.text("生成 AI 字幕", "Generate AI subtitles")
                    } else {
                        language.text("重新生成字幕", "Regenerate subtitles")
                    }))
                    .on_hover_text(language.text(
                        "将当前视频的音频分段发送到配置的模型服务",
                        "Send audio chunks to the configured model service",
                    ))
                    .clicked()
                {
                    *act = Act::GenerateSubtitles;
                    ui.close();
                }
                if self.torrent_selected.is_some() && !self.torrent_progress.finished {
                    ui.small(language.text("只读取已下载音频；数据不足时等待，不抢占播放下载", "Uses downloaded audio only; waits for missing data without competing with playback"));
                }
                if ui
                    .add_enabled(
                        !self.subtitles.is_empty(),
                        egui::Button::new(language.text("导出 SRT…", "Export SRT…")),
                    )
                    .clicked()
                {
                    *act = Act::ExportSubtitles;
                    ui.close();
                }
            });
    }
}
