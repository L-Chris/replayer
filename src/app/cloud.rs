use super::*;
use crate::sync::{self, model::Snapshot};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct State {
    configured: Option<sync::Settings>,
    journal: Option<sync::Journal>,
    job: Option<crate::jobs::Job<sync::webdav::Report>>,
    probe: Option<crate::jobs::Job<()>>,
    requested_secrets: BTreeMap<String, String>,
    path: PathBuf,
    capture_after: Instant,
    progress_after: Instant,
    next: Instant,
    retry_after: Instant,
    probe_settings: Option<sync::Settings>,
    changed: Option<Instant>,
    page: workspace::Page,
    focused: bool,
    pub status: String,
    pub failed: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            configured: None,
            journal: None,
            job: None,
            probe: None,
            requested_secrets: BTreeMap::new(),
            path: PathBuf::new(),
            capture_after: Instant::now(),
            progress_after: Instant::now(),
            next: Instant::now(),
            retry_after: Instant::now(),
            probe_settings: None,
            changed: None,
            page: workspace::Page::Home,
            focused: false,
            status: String::new(),
            failed: false,
        }
    }
}
fn settings_snapshot(settings: &Settings) -> Snapshot {
    let mut data = Snapshot::new();
    for p in &settings.ai.providers {
        data.insert(format!("ai/{}/name", p.id), json!(p.name));
        data.insert(format!("ai/{}/url", p.id), json!(p.base_url));
        for model in &p.models {
            data.insert(
                format!(
                    "ai/{}/model/{}",
                    p.id,
                    crate::comics::digest(model.as_bytes())
                ),
                json!(model),
            );
        }
    }
    if let Value::Object(options) = serde_json::to_value(&settings.llm).unwrap_or_default() {
        for (key, value) in options {
            data.insert(format!("subtitle/{key}"), value);
        }
    }
    data.insert(
        "subtitle/concurrency".into(),
        json!(settings.subtitle_concurrency),
    );
    data.insert(
        "image/selected".into(),
        json!(settings.comics.image_selected),
    );
    data.insert("image/quality".into(), json!(settings.comics.image_quality));
    data.insert("image/target".into(), json!(settings.comics.target));
    data.insert("music/source".into(), json!(settings.music.source));
    data.insert("music/quality".into(), json!(settings.music.quality));
    data
}
fn secrets(settings: &Settings) -> BTreeMap<String, String> {
    settings
        .ai
        .providers
        .iter()
        .map(|p| (format!("ai/{}", p.id), p.api_key.clone()))
        .chain(std::iter::once((
            "music/chksz".into(),
            settings.music.api_key.clone(),
        )))
        .collect()
}

impl App {
    fn cloud_snapshot(&self) -> Snapshot {
        let mut data = settings_snapshot(&self.settings);
        let tracked = |key: &str| {
            self.cloud
                .journal
                .as_ref()
                .is_some_and(|j| j.document.entries.contains_key(key))
        };
        for entry in self.comic.entries() {
            let prefix = format!("comic/{}", entry.book.key());
            if !entry.favorite && !tracked(&format!("{prefix}/favorite")) {
                continue;
            }
            data.insert(
                format!("{prefix}/book"),
                json!({"book":entry.book,"chapters":entry.chapters}),
            );
            data.insert(format!("{prefix}/favorite"), json!(entry.favorite));
            if let Some(progress) = &entry.progress {
                data.insert(
                    format!("{prefix}/progress"),
                    json!({"position":progress,"read_at":entry.last_read}),
                );
            }
            for (set, values) in [("read", &entry.read), ("unread", &entry.new_unread)] {
                for chapter in values {
                    data.insert(
                        format!(
                            "{prefix}/{set}/{}",
                            crate::comics::digest(chapter.as_bytes())
                        ),
                        json!(chapter),
                    );
                }
            }
        }
        for record in &self.library.records {
            let Some(song) = &record.online else { continue };
            let prefix = format!("song/{}", song.key());
            if !record.favorite && !tracked(&format!("{prefix}/favorite")) {
                continue;
            }
            data.insert(format!("{prefix}/song"), json!(song));
            data.insert(format!("{prefix}/favorite"), json!(record.favorite));
            data.insert(format!("{prefix}/progress"), json!({"position":record.position,"duration":record.duration,"played":record.played}));
        }
        data
    }

    fn capture_cloud(&mut self, progress: bool) -> anyhow::Result<bool> {
        if progress {
            self.comic.flush();
        }
        let mut snapshot = self.cloud_snapshot();
        let Some(journal) = &mut self.cloud.journal else {
            return Ok(false);
        };
        if !progress {
            for (key, value) in &mut snapshot {
                if key.ends_with("/progress")
                    && let Some(old) = journal.observed.get(key)
                {
                    *value = old.clone();
                }
            }
        }
        let changed = journal.capture(snapshot, &settings_snapshot(&Settings::default()));
        let keys_changed = self.settings.sync.sync_keys
            && secrets(&self.settings).iter().any(|(id, key)| {
                (!key.is_empty() || journal.secret_hashes.contains_key(id))
                    && journal.secret_hashes.get(id) != Some(&crate::comics::digest(key.as_bytes()))
            });
        if changed {
            journal.save(&self.cloud.path)?;
        }
        Ok(changed || keys_changed)
    }

    pub(super) fn poll_cloud(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if self.cloud.configured.as_ref() != Some(&self.settings.sync) {
            self.cloud.job = None;
            self.cloud.probe = None;
            self.cloud.journal = None;
            self.cloud.configured = Some(self.settings.sync.clone());
            self.cloud.status.clear();
            self.cloud.failed = false;
            self.cloud.changed = None;
            self.cloud.next = now;
            self.cloud.retry_after = now;
            if self.settings.sync.enabled {
                self.cloud.path = Settings::queue_path()
                    .with_file_name("sync")
                    .join(self.settings.sync.account())
                    .join(format!("{}.json", self.settings.sync.device_id));
                match sync::Journal::load(&self.cloud.path, &self.settings.sync.device_id) {
                    Ok(journal) => {
                        self.cloud.journal = Some(journal);
                        self.cloud.failed = false;
                        self.cloud.status.clear();
                    }
                    Err(error) => {
                        self.cloud.failed = true;
                        self.cloud.status = error.to_string();
                    }
                }
            }
        }
        if self
            .cloud
            .probe_settings
            .as_ref()
            .is_some_and(|s| *s != self.preferences_draft.sync)
        {
            self.cloud.probe = None;
            self.cloud.probe_settings = None;
            self.cloud.status.clear();
        }
        if let Some(result) = self.cloud.probe.as_ref().and_then(crate::jobs::Job::poll) {
            self.cloud.probe = None;
            self.cloud.probe_settings = None;
            self.cloud.failed = result.is_err();
            self.cloud.status = result.err().map_or_else(
                || {
                    self.settings
                        .language
                        .text("连接正常", "Connection verified")
                        .into()
                },
                |e| e.to_string(),
            );
        }
        if !self.settings.sync.enabled || self.cloud.journal.is_none() {
            return;
        }
        let page_changed = self.cloud.page != self.workspace.page;
        self.cloud.page = self.workspace.page;
        if now >= self.cloud.capture_after || page_changed {
            let progress = page_changed || now >= self.cloud.progress_after;
            if progress {
                self.cloud.progress_after = now + Duration::from_secs(30);
            }
            match self.capture_cloud(progress) {
                Ok(true) => {
                    self.cloud.changed.get_or_insert(now);
                }
                Ok(false) => {}
                Err(error) => {
                    self.cloud.failed = true;
                    self.cloud.status = error.to_string();
                }
            }
            self.cloud.capture_after = now + Duration::from_secs(1);
        }
        if let Some(result) = self.cloud.job.as_ref().and_then(crate::jobs::Job::poll) {
            self.cloud.job = None;
            match result.and_then(|report| self.finish_cloud(report)) {
                Ok(()) => {
                    self.cloud.failed = false;
                    self.cloud.status = self
                        .settings
                        .language
                        .text("同步完成", "Sync complete")
                        .into();
                }
                Err(error) => {
                    self.cloud.failed = true;
                    self.cloud.status = error.to_string();
                    self.cloud.changed = None;
                    self.cloud.retry_after = now + Duration::from_secs(300);
                }
            }
            self.cloud.next = now + Duration::from_secs(5 * 60);
        }
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let returned = focused && !self.cloud.focused;
        self.cloud.focused = focused;
        let dirty = self
            .cloud
            .changed
            .is_some_and(|since| now.saturating_duration_since(since) >= Duration::from_secs(5));
        if self.cloud.job.is_none()
            && self.cloud.probe.is_none()
            && now >= self.cloud.retry_after
            && (dirty || now >= self.cloud.next && (focused || returned))
        {
            self.begin_cloud(ctx);
        }
    }
    fn begin_cloud(&mut self, ctx: &egui::Context) {
        if self.cloud.job.is_some() || self.cloud.journal.is_none() {
            return;
        }
        if let Err(error) = self.capture_cloud(true).and_then(|_| {
            anyhow::ensure!(
                self.settings.credential_error.is_none(),
                "无法读取本机密钥，已暂停同步 / Credential storage is unavailable; sync paused"
            );
            self.settings.sync.validate()
        }) {
            self.cloud.status = error.to_string();
            self.cloud.failed = true;
            self.cloud.next = Instant::now() + Duration::from_secs(300);
            self.cloud.changed = None;
            return;
        }
        let settings = self.settings.sync.clone();
        let journal = self.cloud.journal.clone().unwrap();
        let keys = secrets(&self.settings);
        self.cloud.requested_secrets = keys.clone();
        self.cloud.changed = None;
        self.cloud.status = self.settings.language.text("正在同步…", "Syncing…").into();
        let ctx = ctx.clone();
        self.cloud.job = Some(crate::jobs::Job::spawn(move |_| async move {
            let result = tokio::time::timeout(
                Duration::from_secs(120),
                sync::webdav::run(settings, journal, keys),
            )
            .await
            .map_err(|_| anyhow::anyhow!("同步超时 / Sync timed out"))
            .and_then(|r| r);
            ctx.request_repaint();
            result
        }));
    }
    fn finish_cloud(&mut self, report: sync::webdav::Report) -> anyhow::Result<()> {
        self.capture_cloud(true)?;
        let mut journal = self.cloud.journal.clone().unwrap();
        journal.merge(&report.journal.document)?;
        let old_settings = self.settings.clone();
        let old_draft = self.preferences_draft.clone();
        let current_keys = secrets(&old_settings);
        let values = journal.values();
        let mut next = old_settings.clone();
        apply_settings(&mut next, &values)?;
        for (id, key) in report.secrets {
            if current_keys.get(&id) != self.cloud.requested_secrets.get(&id) {
                continue;
            }
            if let Some(provider) = id
                .strip_prefix("ai/")
                .and_then(|id| next.ai.providers.iter_mut().find(|p| p.id == id))
            {
                provider.api_key = key;
            } else if id == "music/chksz" {
                next.music.api_key = key;
            }
        }
        next.ai.validate()?;
        if next != old_settings {
            next.save()?;
        }
        self.settings = next;
        if old_draft == old_settings {
            self.preferences_draft = self.settings.clone();
        } else {
            // Rebase unsaved edits so saving a draft cannot undo an unrelated pull.
            let before = settings_snapshot(&old_settings);
            let draft = settings_snapshot(&old_draft);
            let mut rebased = values.clone();
            for key in before.keys().chain(draft.keys()) {
                if before.get(key) != draft.get(key) {
                    if let Some(v) = draft.get(key) {
                        rebased.insert(key.clone(), v.clone());
                    } else {
                        rebased.remove(key);
                    }
                }
            }
            self.preferences_draft = old_draft;
            apply_settings(&mut self.preferences_draft, &rebased)?;
            for p in &mut self.preferences_draft.ai.providers {
                let before = old_settings
                    .ai
                    .providers
                    .iter()
                    .find(|v| v.id == p.id)
                    .map(|v| v.api_key.as_str())
                    .unwrap_or_default();
                if p.api_key == before
                    && let Some(pulled) = self.settings.ai.providers.iter().find(|v| v.id == p.id)
                {
                    p.api_key = pulled.api_key.clone();
                }
            }
            if self.preferences_draft.music.api_key == old_settings.music.api_key {
                self.preferences_draft.music.api_key = self.settings.music.api_key.clone();
            }
        }
        self.apply_cloud_library(&values)?;
        journal.uploaded = report.journal.uploaded;
        journal.last_success = report.journal.last_success;
        journal.secret_hashes = report.journal.secret_hashes;
        self.cloud.journal = Some(journal);
        let observed = self.cloud_snapshot();
        let journal = self.cloud.journal.as_mut().unwrap();
        // Persist normalization (e.g. a read chapter clears its unread flag),
        // otherwise stale remote fields could return on a later synchronization.
        for (key, value) in &values {
            if key.starts_with("secret/") {
                continue;
            }
            if observed.get(key) != Some(value) {
                journal.write(key.clone(), observed.get(key).cloned());
            }
        }
        journal.observed = observed;
        self.cloud
            .journal
            .as_ref()
            .unwrap()
            .save(&self.cloud.path)?;
        if self.cloud.journal.as_ref().unwrap().digest()
            != self.cloud.journal.as_ref().unwrap().uploaded
        {
            self.cloud.changed = Some(Instant::now());
        }
        Ok(())
    }

    fn apply_cloud_library(&mut self, values: &Snapshot) -> anyhow::Result<()> {
        let previous_records = self.library.records.clone();
        for (key, value) in values
            .iter()
            .filter(|(k, _)| k.starts_with("comic/") && k.ends_with("/book"))
        {
            let prefix = key.trim_end_matches("/book");
            let book: crate::comics::Book = serde_json::from_value(value["book"].clone())?;
            anyhow::ensure!(
                prefix == format!("comic/{}", book.key()),
                "Invalid synced comic identity"
            );
            let mut entry = self
                .comic
                .entries()
                .iter()
                .find(|e| e.book.key() == book.key())
                .cloned()
                .unwrap_or_default();
            entry.book = book;
            entry.chapters = serde_json::from_value(value["chapters"].clone())?;
            entry.favorite = values
                .get(&format!("{prefix}/favorite"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Some(progress) = values.get(&format!("{prefix}/progress")) {
                entry.progress = Some(serde_json::from_value(progress["position"].clone())?);
                entry.last_read = progress["read_at"].as_u64().unwrap_or(0);
            }
            entry.read = values
                .iter()
                .filter(|(k, _)| k.starts_with(&format!("{prefix}/read/")))
                .filter_map(|(_, v)| v.as_str().map(str::to_owned))
                .collect();
            entry.new_unread = values
                .iter()
                .filter(|(k, _)| k.starts_with(&format!("{prefix}/unread/")))
                .filter_map(|(_, v)| v.as_str().map(str::to_owned))
                .filter(|id| !entry.read.contains(id))
                .collect();
            entry
                .known
                .extend(entry.chapters.iter().map(|c| c.key().to_owned()));
            let unchanged = self
                .comic
                .entries()
                .iter()
                .find(|e| e.book.key() == entry.book.key())
                .is_some_and(|old| {
                    serde_json::to_value(old).ok() == serde_json::to_value(&entry).ok()
                });
            if !unchanged {
                self.comic.import_synced(entry)?;
            }
        }
        for (key, value) in values
            .iter()
            .filter(|(k, _)| k.starts_with("song/") && k.ends_with("/song"))
        {
            let song: crate::music_sources::OnlineSong = serde_json::from_value(value.clone())?;
            let prefix = key.trim_end_matches("/song");
            anyhow::ensure!(
                prefix == format!("song/{}", song.key()),
                "Invalid synced song identity"
            );
            let path = self
                .library
                .records
                .iter()
                .find(|r| r.online.as_ref().is_some_and(|s| s.key() == song.key()))
                .map(|r| r.path.clone())
                .unwrap_or_else(|| format!("online://{}", song.key()));
            self.library.bind_online(path.clone(), song);
            let record = self
                .library
                .records
                .iter_mut()
                .find(|r| r.path == path)
                .unwrap();
            record.favorite = values
                .get(&format!("{prefix}/favorite"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Some(p) = values.get(&format!("{prefix}/progress")) {
                record.position = p["position"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .unwrap_or(0.0);
                record.duration = p["duration"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .unwrap_or(0.0);
                record.played = p["played"].as_u64().unwrap_or(0);
            }
        }
        if self.library.records != previous_records {
            self.library.save_records()?;
        }
        Ok(())
    }

    pub(super) fn preferences_cloud(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        let settings = &mut self.preferences_draft.sync;
        preferences::form_row(ui, l.text("启用同步", "Enable sync"), l, |ui| {
            design::toggle(ui, &mut settings.enabled, l.text("启用同步", "Enable sync"));
        });
        preferences::form_row(ui, l.text("提供商", "Provider"), l, |ui| {
            let old = settings.provider;
            let selected = if old == sync::Provider::Custom {
                l.text("自定义", "Custom")
            } else {
                old.label()
            };
            egui::ComboBox::from_id_salt("cloud-provider")
                .selected_text(selected)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for provider in sync::Provider::ALL {
                        let name = if provider == sync::Provider::Custom {
                            l.text("自定义", "Custom")
                        } else {
                            provider.label()
                        };
                        ui.selectable_value(&mut settings.provider, provider, name);
                    }
                });
            if settings.provider != old {
                settings.base_url = settings.provider.url().into();
            }
        });
        if settings.provider == sync::Provider::Custom {
            preferences::form_row(ui, l.text("接口地址", "Base URL"), l, |ui| {
                ui.add(design::text_edit(&mut settings.base_url).desired_width(f32::INFINITY));
            });
        }
        for (label, value, password) in [
            (l.text("用户名", "Username"), &mut settings.username, false),
            (l.text("密码", "Password"), &mut settings.password, true),
            (
                l.text("设备名称", "Device name"),
                &mut settings.device_name,
                false,
            ),
        ] {
            preferences::form_row(ui, label, l, |ui| {
                ui.add(
                    design::text_edit(value)
                        .password(password)
                        .desired_width(f32::INFINITY),
                );
            });
        }
        preferences::form_row(ui, l.text("同步 API Key", "Sync API keys"), l, |ui| {
            design::toggle(
                ui,
                &mut settings.sync_keys,
                l.text("同步 API Key", "Sync API keys"),
            );
        });
        if settings.sync_keys {
            preferences::form_row(ui, l.text("同步口令", "Sync passphrase"), l, |ui| {
                ui.add(
                    design::text_edit(&mut settings.passphrase)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text(l.text("至少 12 个字符", "At least 12 characters")),
                );
            });
        }
        ui.add_space(12.0);
        let busy = self.cloud.job.is_some() || self.cloud.probe.is_some();
        let mut test = false;
        let mut sync_now = false;
        preferences::form_row(ui, "", l, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                test = ui.button(l.text("测试连接", "Test connection")).clicked();
            });
            ui.add_enabled_ui(
                !busy
                    && self.settings.sync.enabled
                    && self.preferences_draft.sync == self.settings.sync,
                |ui| {
                    sync_now = workspace::primary(ui, l.text("立即同步", "Sync now")).clicked();
                },
            );
        });
        if test {
            let mut settings = self.preferences_draft.sync.clone();
            self.cloud.probe_settings = Some(settings.clone());
            settings.sync_keys = false;
            let ctx = ui.ctx().clone();
            self.cloud.status = l.text("正在测试连接…", "Testing connection…").into();
            self.cloud.failed = false;
            self.cloud.probe = Some(crate::jobs::Job::spawn(move |_| async move {
                let result = sync::webdav::test(settings).await;
                ctx.request_repaint();
                result
            }));
        }
        if sync_now {
            self.begin_cloud(ui.ctx());
        }
        preferences::form_row(ui, "", l, |ui| {
            if busy {
                ui.spinner();
            }
            if !self.cloud.status.is_empty() {
                ui.colored_label(
                    if self.cloud.failed {
                        Color32::LIGHT_RED
                    } else {
                        MUTED
                    },
                    &self.cloud.status,
                );
            }
        });
        if let Some(journal) = &self.cloud.journal
            && journal.last_success > 0
        {
            let minutes = crate::comics::now().saturating_sub(journal.last_success) / 60;
            preferences::form_row(ui, l.text("最近同步", "Last sync"), l, |ui| {
                ui.label(if minutes == 0 {
                    l.text("刚刚", "Just now").into()
                } else {
                    format!("{minutes} {}", l.text("分钟前", "minutes ago"))
                });
            });
        }
    }
}

fn apply_settings(settings: &mut Settings, values: &Snapshot) -> anyhow::Result<()> {
    let ids: BTreeSet<_> = values
        .keys()
        .filter_map(|k| {
            k.strip_prefix("ai/")?
                .split_once('/')
                .map(|(id, _)| id.to_owned())
        })
        .collect();
    settings.ai.providers = ids
        .into_iter()
        .filter_map(|id| {
            let name = values.get(&format!("ai/{id}/name"))?.as_str()?.to_owned();
            let api_key = settings
                .ai
                .providers
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.api_key.clone())
                .unwrap_or_default();
            let prefix = format!("ai/{id}/model/");
            Some(crate::ai::Provider {
                name,
                base_url: values
                    .get(&format!("ai/{id}/url"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                models: values
                    .iter()
                    .filter(|(k, _)| k.starts_with(&prefix))
                    .filter_map(|(_, v)| v.as_str().map(str::to_owned))
                    .collect(),
                api_key,
                id,
            })
        })
        .collect();
    let mut llm = serde_json::to_value(&settings.llm)?;
    for (key, value) in values
        .iter()
        .filter_map(|(k, v)| k.strip_prefix("subtitle/").map(|s| (s, v)))
    {
        if matches!(
            key,
            "selected"
                | "audio_encoding"
                | "audio_format"
                | "bilingual"
                | "reasoning_effort"
                | "chunk_seconds"
        ) {
            llm[key] = value.clone();
        }
    }
    settings.llm = serde_json::from_value(llm)?;
    if let Some(value) = values.get("subtitle/concurrency") {
        let concurrency = value
            .as_u64()
            .filter(|n| (1..=6).contains(n))
            .ok_or_else(|| anyhow::anyhow!("Invalid synced subtitle concurrency"))?;
        settings.subtitle_concurrency = concurrency as usize;
    }
    if let Some(value) = values.get("image/selected") {
        settings.comics.image_selected = serde_json::from_value(value.clone())?;
    }
    for (key, field) in [
        ("image/quality", &mut settings.comics.image_quality),
        ("image/target", &mut settings.comics.target),
        ("music/source", &mut settings.music.source),
        ("music/quality", &mut settings.music.quality),
    ] {
        if let Some(value) = values.get(key) {
            *field = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid sync setting"))?
                .into();
        }
    }
    for selected in [
        &mut settings.llm.selected,
        &mut settings.comics.image_selected,
    ] {
        if !settings
            .ai
            .providers
            .iter()
            .any(|p| p.id == selected.provider && p.models.contains(&selected.model))
        {
            *selected = Default::default();
        }
    }
    crate::subtitles::LlmConfig::validate_options(&settings.llm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(ctx: &egui::Context, directory: &std::path::Path) -> App {
        let mut app = comic::visual_tests::settings_fixture(ctx);
        app.library = library::Library::open(directory.join("library.json"));
        app
    }
    fn directory() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(uuid::Uuid::new_v4().to_string())
    }
    fn song() -> crate::music_sources::OnlineSong {
        crate::music_sources::OnlineSong {
            source: "chksz-163".into(),
            id: "123456".into(),
            title: "Online song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            cover: None,
        }
    }
    #[test]
    fn portable_snapshot_excludes_local_media_paths_credentials_and_machine_preferences() {
        let ctx = egui::Context::default();
        let directory = directory();
        let mut app = app(&ctx, &directory);
        app.settings.music.directory = "F:/private/downloads".into();
        app.settings.subtitle_concurrency = 4;
        app.settings.sync.password = "private-webdav-password".into();
        app.settings.sync.passphrase = "private-sync-passphrase".into();
        app.settings.music.api_key = "private-music-key".into();
        app.library.records.push(library::Record {
            path: "F:/private/local.flac".into(),
            title: "LocalOnlyUnique".into(),
            music: true,
            favorite: true,
            ..Default::default()
        });
        app.library.records.push(library::Record {
            path: "F:/private/local.mp4".into(),
            title: "VideoOnlyUnique".into(),
            favorite: true,
            ..Default::default()
        });
        app.library
            .bind_online("F:/private/download.flac".into(), song());
        app.library
            .toggle_favorite("F:/private/download.flac", "Online song", true);
        let data = app.cloud_snapshot();
        let text = serde_json::to_string(&data).unwrap();
        for excluded in [
            "F:/private",
            "LocalOnlyUnique",
            "VideoOnlyUnique",
            "private-webdav-password",
            "private-sync-passphrase",
            "private-music-key",
            "not-a-real-key",
        ] {
            assert!(!text.contains(excluded), "{excluded} leaked into sync data");
        }
        assert!(text.contains("Online song"));
        let mut target = Settings {
            language: Language::English,
            ..Default::default()
        };
        target.music.directory = "F:/another-device".into();
        apply_settings(&mut target, &data).unwrap();
        assert!(target.language == Language::English);
        assert_eq!(target.music.directory, "F:/another-device");
        assert_eq!(target.subtitle_concurrency, 4);
        assert!(target.ai.providers.iter().all(|p| p.api_key.is_empty()));
        assert_eq!(target.llm.selected.model, app.settings.llm.selected.model);
        drop(app);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn cloud_song_import_keeps_local_bindings_and_metadata_refresh_does_not_erase_identity() {
        let ctx = egui::Context::default();
        let da = directory();
        let db = directory();
        let mut first = app(&ctx, &da);
        let mut second = app(&ctx, &db);
        let song = song();
        first
            .library
            .bind_online("F:/first/song.flac".into(), song.clone());
        first
            .library
            .toggle_favorite("F:/first/song.flac", "Online song", true);
        first.library.records[0].duration = 240.0;
        first.library.records[0].position = 45.0;
        second.apply_cloud_library(&first.cloud_snapshot()).unwrap();
        assert_eq!(second.library.records.len(), 1);
        assert!(second.library.records[0].path.starts_with("online://"));
        assert_eq!(second.library.records[0].resume_at(), Some(45.0));
        assert!(second.library.records[0].favorite);
        second
            .library
            .bind_online("F:/second/song.flac".into(), song.clone());
        second.apply_cloud_library(&first.cloud_snapshot()).unwrap();
        assert_eq!(second.library.records.len(), 1);
        assert_eq!(second.library.records[0].path, "F:/second/song.flac");
        second.library.remember(library::Record {
            path: "F:/second/song.flac".into(),
            title: "Refreshed".into(),
            music: true,
            ..Default::default()
        });
        assert_eq!(
            second.library.records[0].online.as_ref().unwrap().key(),
            song.key()
        );
        assert!(second.library.records[0].favorite);
        drop((first, second));
        std::fs::remove_dir_all(da).unwrap();
        std::fs::remove_dir_all(db).unwrap();
    }
    #[test]
    fn changing_sync_account_discards_old_tasks_and_unsaved_probe_results() {
        let ctx = egui::Context::default();
        let directory = directory();
        let mut app = app(&ctx, &directory);
        app.cloud.configured = Some(app.settings.sync.clone());
        app.cloud.job = Some(crate::jobs::Job::spawn(|_| async {
            std::future::pending().await
        }));
        app.settings.sync.base_url = "https://new.example/dav".into();
        app.poll_cloud(&ctx);
        assert!(app.cloud.job.is_none());
        assert!(app.cloud.journal.is_none());
        app.cloud.probe_settings = Some(app.preferences_draft.sync.clone());
        app.cloud.probe = Some(crate::jobs::Job::spawn(|_| async {
            std::future::pending().await
        }));
        app.preferences_draft.sync.username = "new-user".into();
        app.poll_cloud(&ctx);
        assert!(app.cloud.probe.is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(directory);
    }
}
