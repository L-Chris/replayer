use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
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

pub(super) struct Library {
    pub records: Vec<Record>,
    dirty: bool,
    saved: Instant,
    path: PathBuf,
}
impl Library {
    fn path() -> PathBuf {
        Settings::queue_path().with_file_name("media-library.json")
    }
    pub fn load() -> Self {
        Self::open(Self::path())
    }
    fn open(path: PathBuf) -> Self {
        Self {
            records: std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default(),
            dirty: false,
            saved: Instant::now(),
            path,
        }
    }
    pub fn remember(&mut self, record: Record) {
        if let Some(old) = self.records.iter_mut().find(|r| r.path == record.path) {
            let favorite = old.favorite;
            *old = record;
            old.favorite = favorite;
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
        } else {
            self.records.push(Record {
                path: path.into(),
                title: title.into(),
                music,
                favorite: true,
                ..Default::default()
            });
        }
        self.dirty = true;
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
    pub(super) fn update_library(&mut self) {
        let Some(source) = self.source.as_ref().filter(|p| p.is_file()) else {
            return;
        };
        let Some(player) = &self.player else { return };
        if self.loading || !player.duration().is_finite() {
            return;
        }
        let path = source.to_string_lossy().into_owned();
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
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn favorites_and_playback_history_survive_restart_and_metadata_refresh() {
        let folder =
            std::env::temp_dir().join(format!("replayer-library-{}", uuid::Uuid::new_v4()));
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
