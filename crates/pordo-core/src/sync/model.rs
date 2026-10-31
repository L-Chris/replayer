use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
pub type Snapshot = BTreeMap<String, Value>;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub value: Option<Value>,
    pub clock: BTreeMap<String, u64>,
    pub time: u64,
    pub author: String,
    // Travels with the winning boolean, including concurrent cancellations.
    // Older clients can still read the boolean value and ignore this metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorited_at: Option<u64>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    pub device: String,
    pub entries: BTreeMap<String, Entry>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Journal {
    pub document: Document,
    pub observed: Snapshot,
    pub uploaded: String,
    pub last_success: u64,
    pub secret_hashes: BTreeMap<String, String>,
}
fn dominates(a: &Entry, b: &Entry) -> bool {
    b.clock
        .iter()
        .all(|(device, count)| a.clock.get(device).unwrap_or(&0) >= count)
        && a.clock
            .iter()
            .any(|(device, count)| count > b.clock.get(device).unwrap_or(&0))
}
fn deletion(entry: &Entry) -> bool {
    entry.value.is_none() || entry.value == Some(Value::Bool(false))
}
impl Journal {
    pub fn write(&mut self, key: String, value: Option<Value>) {
        let old = self.document.entries.get(&key);
        let mut clock = old.map(|e| e.clock.clone()).unwrap_or_default();
        *clock.entry(self.document.device.clone()).or_default() += 1;
        let time = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64)
            .max(old.map_or(0, |e| e.time.saturating_add(1)));
        self.document.entries.insert(
            key,
            Entry {
                value,
                clock,
                time,
                author: self.document.device.clone(),
                favorited_at: None,
            },
        );
    }
    pub fn load(path: &Path, device: &str) -> Result<Self> {
        let mut journal: Self = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        ensure!(
            journal.document.version <= 1,
            "Unsupported sync journal version"
        );
        journal.document.version = 1;
        journal.document.device = device.into();
        Ok(journal)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec(self)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
    pub fn capture(&mut self, snapshot: Snapshot, defaults: &Snapshot) -> bool {
        let mut changed = false;
        let keys: std::collections::BTreeSet<_> = self
            .observed
            .keys()
            .chain(snapshot.keys())
            .cloned()
            .collect();
        for key in keys {
            // Favorite timestamps are metadata on the favorite operation, never
            // separate registers that could select a different conflict winner.
            if key.ends_with("/favorited_at") {
                continue;
            }
            let value = snapshot.get(&key);
            let favorite_time = |data: &Snapshot| {
                data.get(&key.replace("/favorite", "/favorited_at"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            };
            let at = key.ends_with("/favorite").then(|| favorite_time(&snapshot));
            let time_changed = at.is_some_and(|at| at != favorite_time(&self.observed));
            if self.observed.get(&key) == value && !time_changed {
                continue;
            }
            // An empty/new device must not publish defaults over an existing account.
            if !self.observed.contains_key(&key)
                && !self.document.entries.contains_key(&key)
                && defaults.get(&key) == value
            {
                continue;
            }
            self.write(key.clone(), value.cloned());
            self.document.entries.get_mut(&key).unwrap().favorited_at = at;
            changed = true;
        }
        self.observed = snapshot;
        changed
    }
    pub fn merge(&mut self, remote: &Document) -> Result<bool> {
        ensure!(
            remote.version == 1 && remote.entries.len() <= 50_000,
            "Unsupported or oversized sync data"
        );
        let mut changed = false;
        for (key, incoming) in &remote.entries {
            ensure!(
                key.len() <= 2048 && incoming.clock.len() <= 256 && incoming.author.len() <= 128,
                "Invalid sync metadata"
            );
            ensure!(
                incoming.clock.values().all(|n| *n < u64::MAX - 1),
                "Invalid sync counter"
            );
            match self.document.entries.get_mut(key) {
                None => {
                    self.document.entries.insert(key.clone(), incoming.clone());
                    changed = true;
                }
                Some(local) if local != incoming => {
                    let after = dominates(incoming, local);
                    let before = dominates(local, incoming);
                    let winner = if after
                        || !before
                            && (
                                deletion(incoming),
                                incoming.time,
                                &incoming.author,
                                serde_json::to_string(&incoming.value).unwrap_or_default(),
                                incoming.favorited_at,
                            ) > (
                                deletion(local),
                                local.time,
                                &local.author,
                                serde_json::to_string(&local.value).unwrap_or_default(),
                                local.favorited_at,
                            ) {
                        incoming
                    } else {
                        &*local
                    };
                    let mut merged = winner.clone();
                    for (device, count) in local.clock.iter().chain(incoming.clock.iter()) {
                        let n = merged.clock.entry(device.clone()).or_default();
                        *n = (*n).max(*count);
                    }
                    if *local != merged {
                        *local = merged;
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        Ok(changed)
    }
    pub fn values(&self) -> Snapshot {
        let mut values = Snapshot::new();
        for (key, entry) in &self.document.entries {
            if let Some(value) = &entry.value {
                values.insert(key.clone(), value.clone());
                if let Some(prefix) = key.strip_suffix("/favorite") {
                    values.insert(
                        format!("{prefix}/favorited_at"),
                        Value::from(entry.favorited_at.unwrap_or(0)),
                    );
                }
            }
        }
        values
    }
    pub fn digest(&self) -> String {
        crate::comics::digest(&serde_json::to_vec(&self.document).unwrap_or_default())
    }
}
