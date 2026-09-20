use super::*;
use crate::updater::{self, Status};

impl App {
    pub(super) fn about(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let language = self.settings.language;
        ui.add_space(12.0);
        ui.heading("replayer");
        ui.label(format!(
            "{} {} · Windows x64",
            language.text("版本", "Version"),
            env!("CARGO_PKG_VERSION")
        ));
        ui.label(
            RichText::new(
                language.text("音乐 · 视频 · 磁链播放", "Music · Video · Magnet playback"),
            )
            .color(MUTED),
        );
        ui.horizontal(|ui| {
            ui.hyperlink_to(language.text("项目主页", "Project"), updater::REPOSITORY);
            ui.hyperlink_to(
                language.text("发行版本", "Releases"),
                format!("{}/releases", updater::REPOSITORY),
            );
        });
        ui.add_space(8.0);
        ui.separator();
        if ui
            .checkbox(
                &mut self.settings.auto_check_updates,
                language.text("启动时自动检查更新", "Check for updates on startup"),
            )
            .changed()
            && let Err(error) = self.settings.save()
        {
            self.error = Some(error.to_string());
        }
        let status = match &self.updater.status {
            Status::Idle => language
                .text("尚未检查更新", "Updates have not been checked")
                .to_owned(),
            Status::Checking => language
                .text("正在检查更新…", "Checking for updates…")
                .to_owned(),
            Status::Current => language
                .text("暂无更新版本", "No newer release is available")
                .to_owned(),
            Status::Available => language
                .text("发现新版本", "A new version is available")
                .to_owned(),
            Status::Ready => language
                .text(
                    "下载完成，校验通过",
                    "Download verified and ready to install",
                )
                .to_owned(),
            Status::Downloading { received, total } => format!(
                "{} {:.1} / {:.1} MiB",
                language.text("正在下载", "Downloading"),
                *received as f64 / 1048576.0,
                *total as f64 / 1048576.0
            ),
            Status::Failed(error) => {
                format!("{}: {error}", language.text("更新未完成", "Update failed"))
            }
        };
        ui.label(status);
        if let Status::Downloading { received, total } = &self.updater.status {
            ui.add(
                egui::ProgressBar::new(*received as f32 / (*total).max(1) as f32).show_percentage(),
            );
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.updater.busy(),
                    egui::Button::new(language.text("检查更新", "Check for updates")),
                )
                .clicked()
            {
                self.updater.check();
            }
            if self.updater.update.is_some() && !self.updater.busy() {
                if matches!(self.updater.status, Status::Ready) {
                    if ui
                        .button(language.text("安装并重启", "Install and restart"))
                        .clicked()
                        && self.updater.install().is_ok()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                } else if ui
                    .button(language.text("下载更新", "Download update"))
                    .clicked()
                {
                    self.updater.download();
                }
            }
        });
        if !self.subtitles.is_empty() && matches!(self.updater.status, Status::Ready) {
            ui.small(language.text(
                "重启前请导出需要保留的字幕",
                "Export any subtitles you want to keep before restarting",
            ));
        }
        if let Some(update) = &self.updater.update {
            ui.label(RichText::new(format!("v{}", update.version)).strong());
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    let notes =
                        updater::localized_notes(&update.notes, language == Language::Chinese);
                    ui.label(if notes.is_empty() {
                        language.text("此版本暂无更新说明", "No release notes available")
                    } else {
                        notes
                    });
                });
        }
        ui.add_space(10.0);
        ui.small(RichText::new("Rust · FFmpeg · egui / WGPU").color(MUTED));
    }
}
