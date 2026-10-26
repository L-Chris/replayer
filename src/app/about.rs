use super::*;
use crate::updater::{self, Status};

impl App {
    pub(super) fn about(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let language = self.settings.language;
        ui.horizontal(|ui| {
            let (logo, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
            branding::paint(ui.painter(), logo);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("replayer").size(22.0).strong());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(MUTED));
                    ui.hyperlink_to(language.text("项目主页", "Project"), updater::REPOSITORY);
                });
            });
        });
        ui.add_space(4.0);
        ui.separator();
        if !cfg!(windows) {
            ui.hyperlink_to(
                language.text("下载更新", "Download updates"),
                format!("{}/releases/latest", updater::REPOSITORY),
            );
            return;
        }
        preferences::form_row(
            ui,
            language.text("自动检查更新", "Auto-check updates"),
            language,
            |ui| {
                design::toggle(
                    ui,
                    &mut self.preferences_draft.auto_check_updates,
                    language.text("启动时自动检查更新", "Check for updates on startup"),
                );
            },
        );
        let status = match &self.updater.status {
            Status::Idle => String::new(),
            Status::Checking => language
                .text("正在检查更新…", "Checking for updates…")
                .to_owned(),
            Status::Current => language.text("已是最新版本", "Up to date").to_owned(),
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
        if !status.is_empty() {
            preferences::form_row(ui, "", language, |ui| {
                ui.label(RichText::new(status).color(MUTED));
            });
        }
        if let Status::Downloading { received, total } = &self.updater.status {
            preferences::form_row(ui, "", language, |ui| {
                ui.add(
                    egui::ProgressBar::new(*received as f32 / (*total).max(1) as f32)
                        .show_percentage(),
                );
            });
        }
        preferences::form_row(ui, "", language, |ui| {
            let busy = self.updater.busy();
            if ui
                .add_enabled_ui(!busy, |ui| {
                    design::control(
                        ui,
                        language.text("检查更新", "Check updates"),
                        Some(icons::refresh),
                        design::Control::Outline,
                        0.0,
                    )
                })
                .inner
                .clicked()
            {
                self.updater.check();
            }
            if self.updater.update.is_some() && !busy {
                if self.updater.can_install() {
                    if workspace::primary(ui, language.text("安装并重启", "Install and restart"))
                        .clicked()
                        && self.updater.install().is_ok()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                } else if workspace::primary(ui, language.text("下载更新", "Download update"))
                    .clicked()
                {
                    self.updater.download();
                }
            }
            if self.updater.update.is_some()
                && design::icon_button(
                    ui,
                    icons::INFO,
                    language.text("查看更新说明", "View release notes"),
                    false,
                )
                .clicked()
            {
                self.updater.dialog_visible = true;
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
            design::scroll().max_height(180.0).show(ui, |ui| {
                let notes = updater::localized_notes(&update.notes, language == Language::Chinese);
                if notes.is_empty() {
                    ui.label(language.text("此版本暂无更新说明", "No release notes available"));
                } else {
                    release_notes(ui, notes);
                }
            });
        }
    }

    pub(super) fn update_dialog(&mut self, ctx: &egui::Context) {
        if !self.updater.dialog_visible {
            return;
        }
        let Some(update) = self.updater.update.clone() else {
            return;
        };
        let l = self.settings.language;
        let busy = self.updater.busy();
        let ready = self.updater.can_install();
        let mut download = false;
        let mut install = false;
        let mut dismiss = false;
        let modal = egui::Modal::new(Id::new("update-dialog"))
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, design::LINE))
                    .corner_radius(12)
                    .inner_margin(22),
            )
            .show(ctx, |ui| {
                ui.set_width(480.0_f32.min((ctx.content_rect().width() - 64.0).max(240.0)));
                ui.heading(l.text("发现新版本", "Update available"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .color(MUTED)
                            .strong(),
                    );
                    let (arrow_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    icons::paint(ui.painter(), arrow_rect, ACCENT, icons::ARROW_RIGHT);
                    ui.label(
                        RichText::new(format!("v{}", update.version))
                            .color(ACCENT)
                            .strong(),
                    );
                });
                ui.add_space(12.0);
                design::scroll()
                    .id_salt("update-notes")
                    .auto_shrink([false, true])
                    .max_height((ctx.content_rect().height() * 0.4).min(260.0))
                    .show(ui, |ui| {
                        let notes = updater::localized_notes(&update.notes, l == Language::Chinese);
                        if notes.trim().is_empty() {
                            ui.label(l.text("此版本暂无更新说明", "No release notes available"));
                        } else {
                            release_notes(ui, notes);
                        }
                    });
                ui.add_space(14.0);
                match &self.updater.status {
                    Status::Downloading { received, total } => {
                        ui.add(
                            egui::ProgressBar::new(*received as f32 / (*total).max(1) as f32)
                                .show_percentage(),
                        );
                        ui.small(format!(
                            "{} {:.1} / {:.1} MiB",
                            l.text("正在下载", "Downloading"),
                            *received as f64 / 1048576.0,
                            *total as f64 / 1048576.0
                        ));
                        ctx.request_repaint_after(Duration::from_millis(200));
                    }
                    Status::Failed(error) => {
                        ui.colored_label(Color32::LIGHT_RED, l.error(error));
                    }
                    _ => {}
                }
                if ready && !self.subtitles.is_empty() {
                    ui.small(l.text(
                        "重启前请导出需要保留的字幕",
                        "Export subtitles you want to keep before restarting",
                    ));
                }
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !busy {
                        let text = if ready {
                            l.text("安装并重启", "Install and restart")
                        } else if matches!(self.updater.status, Status::Failed(_)) {
                            l.text("重试", "Retry")
                        } else {
                            l.text("下载更新", "Download update")
                        };
                        if design::control(ui, text, None, design::Control::Primary, 0.0).clicked()
                        {
                            install = ready;
                            download = !ready;
                        }
                    }
                    dismiss = design::control(
                        ui,
                        l.text("稍后", "Later"),
                        None,
                        design::Control::Outline,
                        0.0,
                    )
                    .clicked();
                });
            });
        if dismiss || modal.should_close() {
            self.updater.dialog_visible = false;
        }
        if download {
            self.updater.download();
        }
        if install && self.updater.install().is_ok() {
            self.comic.flush();
            self.update_library();
            self.library.flush();
            self.persist_queue();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Release notes use the documented headings, bullet lists and Markdown links.
/// Images and HTML are deliberately not fetched by the in-app notes view.
fn release_notes(ui: &mut Ui, markdown: &str) {
    static LINKS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let links =
        LINKS.get_or_init(|| regex::Regex::new(r"\[([^\]]+)\]\((https?://[^\s)]+)\)").unwrap());
    for line in markdown.lines() {
        let line = line.trim();
        if line.is_empty() {
            ui.add_space(5.0);
        } else if let Some(heading) = line.strip_prefix("## ") {
            ui.label(RichText::new(heading).strong().size(17.0));
        } else if !line.starts_with('<') {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let text = if let Some(item) = line.strip_prefix("- ") {
                    ui.label("•");
                    item
                } else {
                    line
                };
                let mut end = 0;
                for captures in links.captures_iter(text) {
                    let matched = captures.get(0).unwrap();
                    if matched.start() > end {
                        ui.label(text[end..matched.start()].trim());
                    }
                    ui.hyperlink_to(&captures[1], &captures[2]);
                    end = matched.end();
                }
                if end < text.len() {
                    ui.label(text[end..].trim());
                }
            });
        }
    }
}
