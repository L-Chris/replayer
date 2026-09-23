#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    File(String),
    Torrent(usize),
}
#[derive(Clone, Debug)]
pub struct Item {
    pub source: Source,
    pub title: String,
}
impl Item {
    pub fn file(path: String) -> Self {
        let title = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        Self {
            source: Source::File(path),
            title,
        }
    }
}
#[derive(Default)]
pub struct Queue {
    pub items: Vec<Item>,
    pub current: Option<usize>,
}
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct SavedQueue {
    pub items: Vec<String>,
    pub current: Option<usize>,
}
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct SavedQueues {
    pub music: SavedQueue,
    pub video: SavedQueue,
}
impl Queue {
    pub fn saved(&self) -> SavedQueue {
        let mut items = Vec::new();
        let mut current = None;
        for (index, item) in self.items.iter().enumerate() {
            if let Source::File(path) = &item.source {
                if self.current == Some(index) {
                    current = Some(items.len());
                }
                items.push(path.clone());
            }
        }
        SavedQueue { items, current }
    }
    pub fn restore(&mut self, saved: SavedQueue) {
        self.items = saved.items.into_iter().map(Item::file).collect();
        self.current = saved.current.filter(|current| *current < self.items.len());
    }
    pub fn select(&mut self, index: usize) -> Option<Item> {
        let item = self.items.get(index)?.clone();
        self.current = Some(index);
        Some(item)
    }
    pub fn next_index(&self) -> Option<usize> {
        let next = self.current.map_or(0, |i| i + 1);
        (next < self.items.len()).then_some(next)
    }
    pub fn previous_index(&self) -> Option<usize> {
        self.current.and_then(|i| i.checked_sub(1))
    }
    pub fn remove(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.items.remove(index);
        self.current = match self.current {
            Some(current) if current == index => None,
            Some(current) if current > index => Some(current - 1),
            other => other,
        };
    }
    pub fn relocate(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to > self.items.len() || to == from || to == from + 1 {
            return;
        }
        let item = self.items.remove(from);
        let dest = if from < to { to - 1 } else { to };
        self.items.insert(dest, item);
        self.current = self.current.map(|c| {
            if c == from {
                dest
            } else if from < c && dest >= c {
                c - 1
            } else if from > c && dest <= c {
                c + 1
            } else {
                c
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reorder_and_remove_adjust_current() {
        let mut q = Queue {
            items: vec![
                Item::file("a.mp3".into()),
                Item::file("b.mp4".into()),
                Item::file("c.flac".into()),
            ],
            current: None,
        };
        q.select(1).unwrap();
        q.relocate(1, 0);
        assert_eq!(q.current, Some(0));
        q.remove(1);
        assert_eq!(q.items.len(), 2);
        assert_eq!(q.items[1].title, "c.flac");
        assert_eq!(q.current, Some(0));
        q.remove(0);
        assert_eq!(q.items.len(), 1);
        assert_eq!(q.current, None);
        q.select(0);
        assert_eq!(q.next_index(), None);
        assert_eq!(q.previous_index(), None);
    }
}
