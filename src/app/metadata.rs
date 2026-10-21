//! Playback tags and background artwork share a cache, but only a completed
//! background probe counts as an attempt to obtain a video thumbnail.
use crate::media::{Info, Kind};
use crossbeam_channel::Sender;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct Cache {
    entries: HashMap<String, Option<Arc<Info>>>,
    requested: HashSet<String>,
}
impl Cache {
    pub fn get(&self, path: &str) -> Option<&Option<Arc<Info>>> {
        self.entries.get(path)
    }

    pub fn remove(&mut self, path: &str) {
        self.entries.remove(path);
        self.requested.remove(path);
    }

    pub fn request(&mut self, path: &str, sender: &Sender<String>) {
        if self
            .entries
            .get(path)
            .and_then(Option::as_ref)
            .is_some_and(|info| info.kind == Kind::Music || info.artwork.is_some())
            || self.requested.contains(path)
        {
            return;
        }
        self.requested.insert(path.to_owned());
        self.entries.entry(path.to_owned()).or_insert(None);
        if sender.send(path.to_owned()).is_err() {
            self.requested.remove(path);
        }
    }

    pub fn playback(&mut self, path: String, info: Arc<Info>) {
        self.merge(path, Some(info));
    }

    pub fn complete(&mut self, path: String, info: Option<Arc<Info>>) {
        self.requested.insert(path.clone());
        self.merge(path, info);
    }

    fn merge(&mut self, path: String, mut info: Option<Arc<Info>>) {
        if let Some(Some(previous)) = self.entries.get(&path) {
            if let Some(info) = &mut info {
                if info.artwork.is_none() && previous.artwork.is_some() {
                    Arc::make_mut(info).artwork = previous.artwork.clone();
                }
            } else {
                return;
            }
        }
        self.entries.insert(path, info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Artwork;

    fn video(cover: bool) -> Arc<Info> {
        Arc::new(Info {
            kind: Kind::Video,
            title: Some("Video".into()),
            artist: None,
            album: None,
            artwork: cover.then(|| {
                Arc::new(Artwork {
                    width: 1,
                    height: 1,
                    rgba: vec![50, 100, 150, 255],
                })
            }),
        })
    }

    #[test]
    fn playback_tags_do_not_prevent_video_thumbnail_requests_or_erase_results() {
        for playback_first in [true, false] {
            let mut cache = Cache::default();
            let (tx, rx) = crossbeam_channel::unbounded();
            if playback_first {
                cache.playback("video.mkv".into(), video(false));
            }
            cache.request("video.mkv", &tx);
            cache.request("video.mkv", &tx);
            assert_eq!(rx.try_iter().collect::<Vec<_>>(), ["video.mkv"]);
            cache.complete("video.mkv".into(), Some(video(true)));
            cache.playback("video.mkv".into(), video(false));
            cache.complete("video.mkv".into(), None);
            assert!(
                cache
                    .get("video.mkv")
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .artwork
                    .is_some()
            );
            cache.request("video.mkv", &tx);
            assert!(rx.is_empty());
        }
    }

    #[test]
    fn failed_thumbnail_is_not_requeued_each_frame() {
        let mut cache = Cache::default();
        let (tx, rx) = crossbeam_channel::unbounded();
        cache.playback("broken.mkv".into(), video(false));
        cache.request("broken.mkv", &tx);
        assert!(rx.try_recv().is_ok());
        cache.complete("broken.mkv".into(), None);
        for _ in 0..10 {
            cache.request("broken.mkv", &tx);
        }
        assert!(rx.is_empty());
        assert!(cache.get("broken.mkv").unwrap().is_some());
    }
}
