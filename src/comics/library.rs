use super::*;
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub chapter: String,
    pub page: usize,
    pub fraction: f32,
    pub page_count: usize,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub book: Book,
    pub chapters: Vec<Chapter>,
    pub favorite: bool,
    pub progress: Option<Progress>,
    pub last_read: u64,
    pub checked: u64,
    pub check_error: Option<String>,
    pub known: HashSet<String>,
    pub read: HashSet<String>,
    pub new_unread: HashSet<String>,
    pub mode: Option<ReadingMode>,
    pub zoom: f32,
}
impl Entry {
    pub fn refresh(&mut self, detail: Detail) {
        // Unique title matching preserves read state when a source changes URLs.
        for chapter in &detail.chapters {
            if self.known.contains(chapter.key()) {
                continue;
            }
            let old: Vec<_> = self
                .chapters
                .iter()
                .filter(|old| old.name == chapter.name)
                .collect();
            if old.len() == 1
                && detail
                    .chapters
                    .iter()
                    .filter(|c| c.name == chapter.name)
                    .count()
                    == 1
            {
                let old = old[0].key();
                let key = chapter.key().to_owned();
                if self.known.contains(old) {
                    self.known.insert(key.clone());
                }
                if self.read.contains(old) {
                    self.read.insert(key.clone());
                }
                if self.new_unread.remove(old) {
                    self.new_unread.insert(key.clone());
                }
                if let Some(p) = &mut self.progress
                    && p.chapter == old
                {
                    p.chapter = key;
                }
            }
        }
        if self.favorite && !self.known.is_empty() {
            self.new_unread.extend(
                detail
                    .chapters
                    .iter()
                    .filter(|c| !self.known.contains(c.key()) && !self.read.contains(c.key()))
                    .map(|c| c.key().to_owned()),
            );
        }
        self.known
            .extend(detail.chapters.iter().map(|c| c.key().to_owned()));
        let current: HashSet<_> = detail.chapters.iter().map(|c| c.key()).collect();
        self.new_unread.retain(|id| current.contains(id.as_str()));
        self.book = detail.book;
        self.chapters = detail.chapters;
        self.checked = now();
        self.check_error = None;
    }
    pub fn set_favorite(&mut self, favorite: bool) {
        self.favorite = favorite;
        if !favorite {
            self.new_unread.clear();
        }
        self.known
            .extend(self.chapters.iter().map(|c| c.key().to_owned()));
    }
    pub fn mark_read(&mut self, chapter: &str) {
        self.read.insert(chapter.into());
        self.new_unread.remove(chapter);
    }
}
pub struct Library {
    connection: Connection,
    pub entries: Vec<Entry>,
}
impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let connection =
            Connection::open(path).context("无法打开漫画书架 / Cannot open comic library")?;
        connection.busy_timeout(std::time::Duration::from_millis(250))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA user_version=1; CREATE TABLE IF NOT EXISTS entries (id TEXT PRIMARY KEY, payload TEXT NOT NULL);")?;
        let entries = {
            let mut statement = connection.prepare("SELECT payload FROM entries")?;
            let payloads = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            payloads
                .into_iter()
                .map(|s| serde_json::from_str(&s))
                .collect::<serde_json::Result<Vec<Entry>>>()
                .context("漫画书架数据损坏 / Invalid comic library")?
        };
        Ok(Self {
            connection,
            entries,
        })
    }
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.book.key() == key)
    }
    pub fn save(&mut self, entry: Entry) -> Result<()> {
        let key = entry.book.key();
        self.connection.execute("INSERT INTO entries(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![key,serde_json::to_string(&entry)?])?;
        if let Some(existing) = self.entries.iter_mut().find(|e| e.book.key() == key) {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
        Ok(())
    }
    pub fn refresh(&mut self, detail: Detail) -> Result<()> {
        let mut entry = self.get(&detail.book.key()).cloned().unwrap_or_default();
        entry.refresh(detail);
        self.save(entry)
    }
}
