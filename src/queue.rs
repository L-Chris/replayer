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
impl Queue {
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
        if index >= self.items.len() || self.current == Some(index) {
            return;
        }
        self.items.remove(index);
        if let Some(current) = &mut self.current
            && index < *current
        {
            *current -= 1;
        }
    }
    pub fn move_item(&mut self, index: usize, up: bool) {
        let other = if up {
            index.checked_sub(1)
        } else {
            index.checked_add(1)
        };
        let Some(other) = other.filter(|&i| i < self.items.len() && index < self.items.len())
        else {
            return;
        };
        self.items.swap(index, other);
        self.current = self.current.map(|i| {
            if i == index {
                other
            } else if i == other {
                index
            } else {
                i
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reorder_and_remove_preserve_playing_item() {
        let mut q = Queue {
            items: vec![
                Item::file("a.mp3".into()),
                Item::file("b.mp4".into()),
                Item::file("c.flac".into()),
            ],
            current: None,
        };
        q.select(1).unwrap();
        q.move_item(1, true);
        assert_eq!(q.current, Some(0));
        q.remove(0);
        assert_eq!(q.items.len(), 3);
        q.remove(1);
        assert_eq!(q.items[0].title, "b.mp4");
        assert_eq!(q.next_index(), Some(1));
        q.select(1);
        assert_eq!(q.next_index(), None);
        q.remove(0);
        assert_eq!(q.current, Some(0));
        assert_eq!(q.previous_index(), None);
    }
}
