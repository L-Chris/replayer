use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Record {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub music: bool,
    pub position: f64,
    pub duration: f64,
    pub played: u64,
    pub favorite: bool,
    pub favorited_at: u64,
    pub online: Option<crate::music_sources::OnlineSong>,
}
impl Record {
    pub fn resume_at(&self) -> Option<f64> {
        (self.position.is_finite()
            && self.duration.is_finite()
            && self.position > 1.0
            && self.position < self.duration - 2.0)
            .then_some(self.position)
    }
}

pub(super) fn favorite_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn sort_favorites(records: &mut [Record]) {
    records.sort_by_cached_key(|r| {
        (
            std::cmp::Reverse(r.favorited_at),
            r.online
                .as_ref()
                .map(|s| s.key())
                .unwrap_or_else(|| r.path.clone()),
        )
    });
}

pub(super) struct Library {
    pub records: Vec<Record>,
    dirty: bool,
    saved: Instant,
    path: PathBuf,
    suppressed: Option<String>,
}

pub(super) struct RemovedHistory {
    path: String,
    played: u64,
    position: f64,
}
impl Library {
    fn path() -> PathBuf {
        Settings::queue_path().with_file_name("media-library.json")
    }
    pub fn load() -> Self {
        Self::open(Self::path())
    }
    pub(super) fn open(path: PathBuf) -> Self {
        Self {
            records: std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default(),
            dirty: false,
            saved: Instant::now(),
            path,
            suppressed: None,
        }
    }
    pub fn remember(&mut self, record: Record) {
        if self.suppressed.as_deref() == Some(&record.path) {
            return;
        }
        if let Some(old) = self.records.iter_mut().find(|r| {
            r.path == record.path
                || r.online
                    .as_ref()
                    .zip(record.online.as_ref())
                    .is_some_and(|(a, b)| a.key() == b.key())
        }) {
            let favorite = old.favorite;
            let favorited_at = old.favorited_at;
            let online = old.online.clone();
            *old = record;
            old.favorite = favorite;
            old.favorited_at = favorited_at;
            if old.online.is_none() {
                old.online = online;
            }
        } else {
            self.records.push(record);
        }
        self.dirty = true;
        if self.saved.elapsed() >= Duration::from_secs(5) {
            self.flush();
        }
    }
    pub fn toggle_favorite(&mut self, path: &str, title: &str, music: bool) {
        if let Some(record) = self.records.iter_mut().find(|r| r.path == path) {
            record.favorite = !record.favorite;
            if record.favorite {
                record.favorited_at = favorite_time().max(record.favorited_at.saturating_add(1));
            }
        } else {
            self.records.push(Record {
                path: path.into(),
                title: title.into(),
                music,
                favorite: true,
                favorited_at: favorite_time(),
                ..Default::default()
            });
        }
        self.dirty = true;
        self.flush();
    }
    pub fn begin_playback(&mut self) {
        self.suppressed = None;
    }
    pub fn bind_online(&mut self, path: String, song: crate::music_sources::OnlineSong) {
        let key = song.key();
        if let Some(old) = self
            .records
            .iter_mut()
            .find(|r| r.online.as_ref().is_some_and(|s| s.key() == key))
        {
            if old.path == path && old.online.as_ref() == Some(&song) {
                return;
            }
            old.path = path;
            old.title = song.title.clone();
            old.artist = song.artist.clone();
            old.album = song.album.clone();
            old.online = Some(song);
        } else if let Some(old) = self.records.iter_mut().find(|r| r.path == path) {
            old.online = Some(song);
        } else {
            self.records.push(Record {
                path,
                title: song.title.clone(),
                artist: song.artist.clone(),
                album: song.album.clone(),
                music: true,
                online: Some(song),
                ..Default::default()
            });
        }
        self.dirty = true;
        self.flush();
    }
    pub fn save_records(&mut self) -> anyhow::Result<()> {
        self.dirty = true;
        self.flush();
        anyhow::ensure!(
            !self.dirty,
            "无法保存媒体资料库 / Cannot save media library"
        );
        Ok(())
    }
    pub fn remove_history(&mut self, path: &str, active: bool) -> Option<RemovedHistory> {
        let record = self
            .records
            .iter_mut()
            .find(|r| r.path == path && r.played > 0)?;
        let removed = RemovedHistory {
            path: path.into(),
            played: record.played,
            position: record.position,
        };
        record.played = 0;
        record.position = 0.0;
        if active {
            self.suppressed = Some(path.into());
        }
        self.dirty = true;
        self.flush();
        Some(removed)
    }
    pub fn restore_history(&mut self, removed: RemovedHistory) {
        if let Some(record) = self
            .records
            .iter_mut()
            .find(|r| r.path == removed.path && r.played == 0)
        {
            record.played = removed.played;
            record.position = removed.position;
            self.dirty = true;
        }
        if self.suppressed.as_deref() == Some(&removed.path) {
            self.suppressed = None;
        }
        self.flush();
    }
    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        let path = &self.path;
        let result = (|| -> anyhow::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, serde_json::to_vec(&self.records)?)?;
            std::fs::rename(temporary, path)?;
            Ok(())
        })();
        if result.is_ok() {
            self.dirty = false;
            self.saved = Instant::now();
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        self.flush();
    }
}

impl App {
    pub(super) fn active_record_path(&self) -> Option<String> {
        if let Some(song) = &self.online_origin {
            return Some(
                self.library
                    .records
                    .iter()
                    .find(|r| r.online.as_ref().is_some_and(|s| s.key() == song.key()))
                    .map(|r| r.path.clone())
                    .unwrap_or_else(|| format!("online://{}", song.key())),
            );
        }
        self.source
            .as_ref()
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned())
    }

    pub(super) fn remove_history(&mut self, path: &str) {
        let active = self.active_record_path().as_deref() == Some(path);
        if let Some(removed) = self.library.remove_history(path, active) {
            self.history_undo = Some((removed, Instant::now()));
            if self
                .workspace
                .resume
                .as_ref()
                .is_some_and(|(p, _)| p == path)
            {
                self.workspace.resume = None;
            }
        }
    }

    pub(super) fn history_notice(&mut self, ctx: &egui::Context) {
        let Some((_, created)) = &self.history_undo else {
            return;
        };
        let remaining = Duration::from_secs(5).saturating_sub(created.elapsed());
        if remaining.is_zero() {
            self.history_undo = None;
            return;
        }
        ctx.request_repaint_after(remaining);
        let l = self.settings.language;
        let mut undo = false;
        egui::Area::new(Id::new("history-undo"))
            .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-20.0, -112.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(SURFACE)
                    .corner_radius(8)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(l.text("已删除播放记录", "Playback history removed"));
                            undo = ui.button(l.text("撤销", "Undo")).clicked();
                        });
                    });
            });
        if undo && let Some((removed, _)) = self.history_undo.take() {
            self.library.restore_history(removed);
        }
    }

    pub(super) fn update_library(&mut self) {
        let Some(path) = self.active_record_path() else {
            return;
        };
        let Some(player) = &self.player else { return };
        if self.loading || !player.duration().is_finite() {
            return;
        }
        let position = self
            .pending_seek
            .map_or_else(|| player.position(), |(_, t)| t);
        let previous = self.library.records.iter().find(|r| r.path == path);
        if previous.is_some_and(|r| (r.position - position).abs() < 1.0 && r.title == self.title) {
            return;
        }
        let info = self.media_info.as_ref();
        self.library.remember(Record {
            path,
            title: self.title.clone(),
            artist: info.and_then(|i| i.artist.clone()).unwrap_or_default(),
            album: info.and_then(|i| i.album.clone()).unwrap_or_default(),
            music: self.is_music(),
            position,
            duration: player.duration(),
            played: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            favorite: false,
            favorited_at: 0,
            online: self
                .online_origin
                .clone()
                .or_else(|| previous.and_then(|r| r.online.clone())),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn favorite_timestamps_survive_metadata_binding_history_and_restart() {
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(uuid::Uuid::new_v4().to_string());
        let path = folder.join("library.json");
        let mut library = Library::open(path.clone());
        library.toggle_favorite("old.flac", "Old", true);
        let at = library.records[0].favorited_at;
        assert!(at > 0);
        library.remember(Record {
            path: "old.flac".into(),
            title: "Refreshed".into(),
            played: u64::MAX,
            music: true,
            ..Default::default()
        });
        library.bind_online(
            "old.flac".into(),
            crate::music_sources::OnlineSong {
                source: "chksz-163".into(),
                id: "1".into(),
                ..Default::default()
            },
        );
        library.records.push(Record {
            path: "new.mp4".into(),
            favorited_at: at + 1,
            favorite: true,
            ..Default::default()
        });
        library.save_records().unwrap();
        drop(library);
        let mut library = Library::open(path);
        sort_favorites(&mut library.records);
        assert_eq!(library.records[0].path, "new.mp4");
        assert_eq!(library.records[1].favorited_at, at);
        library.toggle_favorite("old.flac", "Old", true);
        library.toggle_favorite("old.flac", "Old", true);
        assert!(library.records[1].favorited_at > at);
        let old: Record = serde_json::from_str(r#"{"favorite":true}"#).unwrap();
        assert_eq!(old.favorited_at, 0);
        drop(library);
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn deleting_history_persists_keeps_favorites_and_suppresses_only_current_session() {
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(uuid::Uuid::new_v4().to_string());
        let path = folder.join("library.json");
        let mut library = Library::open(path.clone());
        let original = Record {
            path: "song.flac".into(),
            title: "Track".into(),
            artist: "Artist".into(),
            music: true,
            position: 60.0,
            duration: 240.0,
            played: 100,
            ..Default::default()
        };
        library.remember(original.clone());
        library.toggle_favorite(&original.path, &original.title, true);
        let removed = library.remove_history(&original.path, true).unwrap();
        library.remember(Record {
            position: 70.0,
            played: 101,
            ..original.clone()
        });
        assert_eq!(library.records[0].played, 0);
        assert!(library.records[0].resume_at().is_none());
        assert!(library.records[0].favorite);
        let persisted = Library::open(path.clone());
        assert_eq!(persisted.records[0].played, 0);
        assert_eq!(persisted.records[0].artist, "Artist");
        drop(persisted);
        // Undo restores history without rolling back independent favorite changes.
        library.toggle_favorite(&original.path, &original.title, true);
        library.restore_history(removed);
        assert_eq!(library.records[0].resume_at(), Some(60.0));
        assert!(!library.records[0].favorite);
        library.remove_history(&original.path, true).unwrap();
        library.begin_playback();
        library.remember(original);
        assert_eq!(library.records[0].played, 100);
        drop(library);
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn favorites_and_playback_history_survive_restart_and_metadata_refresh() {
        let folder = std::env::temp_dir().join(format!("pordo-library-{}", uuid::Uuid::new_v4()));
        let path = folder.join("library.json");
        {
            let mut library = Library::open(path.clone());
            library.toggle_favorite("song.flac", "filename", true);
            library.remember(Record {
                path: "song.flac".into(),
                title: "Track title".into(),
                artist: "Artist".into(),
                position: 74.0,
                duration: 251.0,
                played: 5,
                music: true,
                ..Default::default()
            });
        }
        let library = Library::open(path);
        assert_eq!(library.records.len(), 1);
        assert!(library.records[0].favorite);
        assert_eq!(library.records[0].title, "Track title");
        assert_eq!(library.records[0].resume_at(), Some(74.0));
        drop(library);
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn completed_and_invalid_playback_positions_do_not_resume() {
        for position in [0.0, -10.0, 249.0, 251.0, f64::NAN, f64::INFINITY] {
            assert!(
                Record {
                    position,
                    duration: 251.0,
                    ..Default::default()
                }
                .resume_at()
                .is_none()
            );
        }
    }
}
