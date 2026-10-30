use super::*;

#[derive(Default)]
pub(super) struct State {
    pub job: Option<Job>,
    pub cues: Vec<Cue>,
    owner: Option<PathBuf>,
    source: Option<PathBuf>,
    pub status: String,
    skipped: Vec<(f64, f64, String)>,
    saving: Option<Receiver<Result<PathBuf, String>>>,
}

impl App {
    // Streaming tracks become eligible once their complete local download is
    // available. This avoids requesting uncached audio ahead of playback.
    fn lyrics_source(&self) -> Option<PathBuf> {
        self.source.as_ref()?;
        self.source
            .as_ref()
            .filter(|path| path.is_file())
            .cloned()
            .or_else(|| {
                self.active_record_path()
                    .map(PathBuf::from)
                    .filter(|p| p.is_file())
            })
    }

    pub(super) fn lyrics_menu(&mut self, ui: &mut Ui, act: &mut Act) {
        let language = self.settings.language;
        let generating = self.lyric_generation.job.is_some();
        let ready = !self.lyrics.lines.is_empty();
        let can_generate = self.is_music() && self.lyrics_source().is_some();
        let caption = if generating {
            language.text("AI 歌词 · 生成中", "AI lyrics · generating")
        } else {
            language.text("歌词", "Lyrics")
        };
        let active = generating
            || (self.workspace.page == workspace::Page::Listening
                && self.workspace.listening_tab == 0);
        let response = design::icon_button_sized(ui, 28.0, 1.0, active, caption, draw_subtitles);
        if generating {
            ui.painter()
                .circle_filled(response.rect.right_top() + vec2(-5.0, 5.0), 3.0, ACCENT);
        }
        egui::Popup::menu(&response)
            .align(egui::emath::RectAlign::TOP_END)
            .gap(8.0)
            .show(|ui| {
                ui.set_min_width(220.0);
                ui.set_max_width(320.0);
                if ui.add_enabled(self.source.is_some(), egui::Button::new(language.text("显示歌词", "Show lyrics"))).clicked() {
                    self.workspace.listening_tab = 0;
                    self.workspace.page = workspace::Page::Listening;
                    ui.close();
                }
                ui.separator();
                if !self.lyric_generation.status.is_empty() {
                    ui.label(&self.lyric_generation.status);
                    ui.separator();
                }
                if !self.lyric_generation.skipped.is_empty() {
                    ui.collapsing(language.text("失败的片段", "Failed segments"), |ui| {
                        design::scroll().max_height(160.0).show(ui, |ui| {
                            for (start, end, error) in &self.lyric_generation.skipped {
                                ui.label(format!("{}–{}", fmt_time(*start), fmt_time(*end)));
                                ui.small(language.error(error));
                            }
                        });
                    });
                }
                if generating {
                    if ui.button(language.text("取消生成", "Cancel generation")).clicked() {
                        *act = Act::CancelLyrics;
                        ui.close();
                    }
                } else {
                    if ui.add_enabled(can_generate && self.lyric_generation.saving.is_none(), egui::Button::new(if ready {
                        language.text("重新生成歌词", "Regenerate lyrics")
                    } else {
                        language.text("生成 AI 歌词", "Generate AI lyrics")
                    })).clicked() {
                        *act = Act::GenerateLyrics;
                        ui.close();
                    }
                    if !can_generate {
                        ui.small(language.text(
                            "打开本地歌曲，或等待在线歌曲下载完成后生成",
                            "Open a local song or wait for the online download to finish",
                        ));
                    }
                }
                ui.separator();
                ui.small(language.text(
                    "使用 AI 字幕中选定的音频模型，保留歌曲原文。完成后自动保存，重新生成会覆盖已有歌词。",
                    "Uses the audio model selected for AI subtitles and preserves the original language. Automatically saves on completion; regenerating replaces existing lyrics.",
                ));
            });
    }

    pub(super) fn generate_lyrics(&mut self) {
        if !self.is_music() || self.lyric_generation.saving.is_some() {
            return;
        }
        let Some(source) = self.lyrics_source() else {
            return;
        };
        let language = self.settings.language;
        let result = subtitles::LlmConfig::load_with(&self.settings.llm, &self.settings.ai)
            .and_then(|config| {
                Job::start_with_cache(
                    source.clone(),
                    subtitles::Options {
                        mode: subtitles::Mode::Lyrics,
                        language,
                        concurrency: self.settings.subtitle_concurrency,
                        ..Default::default()
                    },
                    0.0,
                    config,
                    None,
                )
            });
        match result {
            Ok(job) => {
                self.lyric_generation = State {
                    job: Some(job),
                    owner: self.source.clone(),
                    source: Some(source),
                    status: language.text("正在准备音轨…", "Preparing audio…").into(),
                    ..Default::default()
                };
                self.lyrics_follow = true;
                self.workspace.page = workspace::Page::Listening;
                self.workspace.listening_tab = 0;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!(
                    "{}: {error}",
                    language.text("无法启动歌词生成", "Cannot start lyric generation",)
                ))
            }
        }
    }

    pub(super) fn poll_lyrics(&mut self, ctx: &egui::Context) {
        if self.lyric_generation.owner.is_some() && self.lyric_generation.owner != self.source {
            self.lyric_generation = Default::default();
        }
        let events: Vec<_> = self
            .lyric_generation
            .job
            .as_ref()
            .map(|job| job.events.try_iter().collect())
            .unwrap_or_default();
        let language = self.settings.language;
        let mut save = false;
        let state = &mut self.lyric_generation;
        for event in events {
            match event {
                subtitles::Event::Cues(cues) => {
                    state.cues.extend(cues);
                    self.lyrics = crate::lyrics::from_cues(&state.cues);
                }
                subtitles::Event::Progress { through, total } => {
                    state.status = if total > 0.0 {
                        format!(
                            "{} · {:.0}%",
                            language.text("正在生成歌词", "Generating lyrics"),
                            (through / total * 100.0).min(100.0)
                        )
                    } else {
                        format!(
                            "{} · {}",
                            language.text("正在生成歌词", "Generating lyrics"),
                            fmt_time(through)
                        )
                    };
                }
                subtitles::Event::Skipped { start, end, error } => {
                    state.skipped.push((start, end, error))
                }
                subtitles::Event::Finished => {
                    state.job = None;
                    state.status = if state.cues.is_empty() {
                        language
                            .text(
                                "未识别到歌词，保留原有歌词",
                                "No lyrics detected; existing lyrics kept",
                            )
                            .into()
                    } else {
                        format!(
                            "{} · {} {}",
                            language.text("歌词已生成", "Lyrics generated"),
                            self.lyrics.lines.len(),
                            language.text("行", "lines")
                        )
                    };
                    if !state.skipped.is_empty() {
                        state.status.push_str(&format!(
                            " · {} {}",
                            state.skipped.len(),
                            language.text(
                                "段失败，未覆盖已保存歌词，请重新生成",
                                "failed segments; saved lyrics kept. Regenerate to retry."
                            )
                        ));
                    } else if !state.cues.is_empty() {
                        save = true;
                    }
                }
                subtitles::Event::Failed(error) => {
                    state.job = None;
                    state.status = language
                        .text(
                            "歌词生成失败，保留已保存的歌词，可重新生成",
                            "Lyric generation failed; saved lyrics kept. Regenerate to retry.",
                        )
                        .into();
                    self.error = Some(error);
                }
                subtitles::Event::WaitingCache | subtitles::Event::Metrics { .. } => {}
            }
        }
        if let Some(result) = state.saving.as_ref().and_then(|rx| rx.try_recv().ok()) {
            state.saving = None;
            match result {
                Ok(path) => {
                    state.status = format!(
                        "{}: {}",
                        language.text("歌词已自动保存", "Lyrics saved automatically"),
                        path.display()
                    )
                }
                Err(error) => {
                    state.status = language
                        .text(
                            "歌词已生成，但自动保存失败",
                            "Lyrics generated, but automatic saving failed",
                        )
                        .into();
                    self.error = Some(error);
                }
            }
        }
        if state.job.is_some() || state.saving.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if save {
            self.save_lyrics();
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save_lyrics(&mut self) {
        let state = &mut self.lyric_generation;
        if state.cues.is_empty() || state.job.is_some() || state.saving.is_some() {
            return;
        }
        let Some(source) = state.source.clone() else {
            return;
        };
        let lyrics = crate::lyrics::from_cues(&state.cues);
        let language = self.settings.language;
        let (tx, rx) = unbounded();
        state.saving = Some(rx);
        state.status = language
            .text("正在自动保存歌词…", "Saving lyrics automatically…")
            .into();
        thread::spawn(move || {
            let result = lyrics.save_for(&source).map_err(|e| {
                format!(
                    "{}: {}: {e}",
                    language.text("自动保存歌词失败", "Failed to save lyrics automatically"),
                    source.with_extension("lrc").display()
                )
            });
            let _ = tx.send(result);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subtitles::Event;

    fn cue(time: f64, text: &str) -> Cue {
        Cue {
            start: time,
            end: time + 1.0,
            text: text.into(),
            source: None,
        }
    }

    #[test]
    fn lyric_generation_preserves_existing_lyrics_on_empty_and_failed_results() {
        let ctx = egui::Context::default();
        let mut app = comic::visual_tests::settings_fixture(&ctx);
        app.source = Some("song.flac".into());
        for terminal in [Event::Finished, Event::Failed("test failure".into())] {
            app.lyrics = crate::lyrics::Lyrics::parse("[00:01.00]Existing lyrics");
            let (job, _) = Job::test_events([terminal]);
            app.lyric_generation = State {
                job: Some(job),
                owner: app.source.clone(),
                ..Default::default()
            };
            app.poll_lyrics(&ctx);
            assert!(app.lyric_generation.job.is_none());
            assert_eq!(app.lyrics.lines[0].text, "Existing lyrics");
        }
    }

    #[test]
    fn partial_lyrics_remain_visible_after_cancel_and_are_discarded_on_track_change() {
        let ctx = egui::Context::default();
        let mut app = comic::visual_tests::settings_fixture(&ctx);
        app.source = Some("song.flac".into());
        let (job, cancelled) = Job::test_events([
            Event::Cues(vec![cue(15.0, "君の声"), cue(1.0, "Hello, 世界！")]),
            Event::Skipped {
                start: 20.0,
                end: 25.0,
                error: "test failure".into(),
            },
        ]);
        app.lyric_generation = State {
            job: Some(job),
            owner: app.source.clone(),
            ..Default::default()
        };
        app.poll_lyrics(&ctx);
        assert_eq!(app.lyrics.lines[0].text, "Hello, 世界！");
        assert_eq!(app.lyrics.current_index(15.0), Some(1));
        assert_eq!(app.lyric_generation.skipped.len(), 1);
        app.apply_action(Act::CancelLyrics, &ctx);
        assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));
        assert!(app.lyric_generation.job.is_none());
        assert_eq!(
            crate::lyrics::from_cues(&app.lyric_generation.cues).to_lrc(),
            "[00:01.00]Hello, 世界！\n[00:15.00]君の声\n"
        );

        let (job, cancelled) = Job::test_events([Event::Cues(vec![cue(30.0, "Old song")])]);
        app.lyric_generation.job = Some(job);
        app.source = Some("new-song.flac".into());
        app.lyrics = crate::lyrics::Lyrics::parse("[00:01.00]New song");
        app.poll_lyrics(&ctx);
        assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));
        assert!(app.lyric_generation.cues.is_empty());
        assert_eq!(app.lyrics.lines[0].text, "New song");
    }

    #[test]
    fn completed_lyrics_save_automatically_and_only_successful_regeneration_overwrites() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("song.flac");
        let ctx = egui::Context::default();
        let mut app = comic::visual_tests::settings_fixture(&ctx);
        app.source = Some(source.clone());
        for text in ["First generation", "君の声が聞こえる"] {
            let (job, _) = Job::test_events([Event::Cues(vec![cue(1.25, text)]), Event::Finished]);
            app.lyric_generation = State {
                job: Some(job),
                owner: app.source.clone(),
                source: Some(source.clone()),
                ..Default::default()
            };
            app.poll_lyrics(&ctx);
            let deadline = Instant::now() + Duration::from_secs(3);
            while app.lyric_generation.saving.is_some() {
                assert!(Instant::now() < deadline, "Automatic save did not finish");
                thread::sleep(Duration::from_millis(5));
                app.poll_lyrics(&ctx);
            }
            assert!(app.error.is_none(), "{:?}", app.error);
            let loaded = crate::lyrics::Lyrics::load_for(&source).unwrap();
            assert_eq!(loaded.lines[0].text, text);
            assert_eq!(loaded.lines[0].time, 1.25);
        }
        let saved = std::fs::read(source.with_extension("lrc")).unwrap();
        for terminal in [
            vec![Event::Finished],
            vec![
                Event::Cues(vec![cue(1.0, "Partial")]),
                Event::Skipped {
                    start: 5.0,
                    end: 10.0,
                    error: "test failure".into(),
                },
                Event::Finished,
            ],
            vec![
                Event::Cues(vec![cue(1.0, "Partial")]),
                Event::Failed("test failure".into()),
            ],
        ] {
            let (job, _) = Job::test_events(terminal);
            app.lyric_generation = State {
                job: Some(job),
                owner: app.source.clone(),
                source: Some(source.clone()),
                ..Default::default()
            };
            app.poll_lyrics(&ctx);
            assert!(app.lyric_generation.saving.is_none());
            assert_eq!(std::fs::read(source.with_extension("lrc")).unwrap(), saved);
        }
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            1,
            "No temporary files remain"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_automatic_save_keeps_lyrics_visible_and_reports_the_error() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(uuid::Uuid::new_v4().to_string());
        let source = directory.join("song.flac");
        let target = source.with_extension("lrc");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"keep").unwrap();
        let ctx = egui::Context::default();
        let mut app = comic::visual_tests::settings_fixture(&ctx);
        app.source = Some(source.clone());
        let (job, _) = Job::test_events([
            Event::Cues(vec![cue(1.0, "Recognized lyrics")]),
            Event::Finished,
        ]);
        app.lyric_generation = State {
            job: Some(job),
            owner: app.source.clone(),
            source: Some(source),
            ..Default::default()
        };
        app.poll_lyrics(&ctx);
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.lyric_generation.saving.is_some() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
            app.poll_lyrics(&ctx);
        }
        assert_eq!(app.lyrics.lines[0].text, "Recognized lyrics");
        assert!(app.error.is_some());
        assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
