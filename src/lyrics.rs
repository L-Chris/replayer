//! Timed LRC lyrics for the music view. Downloads save a `<stem>.lrc` sidecar
//! next to the audio; playback reads it back for local and online tracks alike.
use std::path::Path;

pub struct LyricLine {
    pub time: f64,
    pub text: String,
}
#[derive(Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub synced: bool,
}
fn parse_time(tag: &str) -> Option<f64> {
    let (minutes, seconds) = tag.trim().split_once(':')?;
    let minutes: f64 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().parse().ok()?;
    (minutes.is_finite() && seconds.is_finite() && minutes >= 0.0 && seconds >= 0.0)
        .then_some(minutes * 60.0 + seconds)
}
impl Lyrics {
    pub fn parse(text: &str) -> Self {
        let mut timed = Vec::new();
        let mut plain = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let mut times = Vec::new();
            let mut rest = line;
            while let Some(stripped) = rest.strip_prefix('[') {
                let Some(close) = stripped.find(']') else {
                    break;
                };
                let Some(time) = parse_time(&stripped[..close]) else {
                    break;
                };
                times.push(time);
                rest = &stripped[close + 1..];
            }
            let text = rest.trim();
            if !times.is_empty() {
                timed.extend(times.into_iter().filter(|_| !text.is_empty()).map(|time| {
                    LyricLine {
                        time,
                        text: text.to_owned(),
                    }
                }));
            } else if !line.starts_with('[') {
                // Plain text lyric without timestamps; keep display order.
                plain.push(LyricLine {
                    time: 0.0,
                    text: text.to_owned(),
                });
            }
            // Remaining bracket lines are LRC tags such as [ar:Artist]; skip them.
        }
        if timed.is_empty() {
            return Self {
                lines: plain,
                synced: false,
            };
        }
        timed.sort_by(|a, b| a.time.total_cmp(&b.time));
        Self {
            lines: timed,
            synced: true,
        }
    }
    /// Index of the line being sung at `position`, or None before the first stamp.
    pub fn current_index(&self, position: f64) -> Option<usize> {
        self.synced
            .then(|| self.lines.partition_point(|line| line.time <= position))
            .and_then(|count| count.checked_sub(1))
    }
    pub fn load_for(path: &Path) -> Option<Self> {
        let sidecar = path.with_extension("lrc");
        let companion = crate::media::companion_path(path, ".lrc");
        [sidecar, companion].into_iter().find_map(|candidate| {
            let bytes = std::fs::read(candidate).ok()?;
            if bytes.len() > 512 * 1024 {
                return None;
            }
            let lyrics = Self::parse(&String::from_utf8_lossy(&bytes));
            (!lyrics.lines.is_empty()).then_some(lyrics)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps_multiple_stamps_and_skips_tags() {
        let lyrics = Lyrics::parse(
            "[ti:Title]\n[ar:Artist]\n[00:12.50]second\n[00:01.00][00:20.00]first\n[00:30.00]\nplain",
        );
        assert!(lyrics.synced);
        let pairs: Vec<(f64, &str)> = lyrics
            .lines
            .iter()
            .map(|line| (line.time, line.text.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![(1.0, "first"), (12.5, "second"), (20.0, "first")]
        );
    }

    #[test]
    fn unsynced_text_keeps_order_and_has_no_current_line() {
        let lyrics = Lyrics::parse("line one\nline two");
        assert!(!lyrics.synced);
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.current_index(10.0), None);
    }

    #[test]
    fn current_index_tracks_position() {
        let lyrics = Lyrics::parse("[00:05.00]a\n[00:10.00]b\n[01:00.00]c");
        assert_eq!(lyrics.current_index(0.0), None);
        assert_eq!(lyrics.current_index(5.0), Some(0));
        assert_eq!(lyrics.current_index(9.99), Some(0));
        assert_eq!(lyrics.current_index(10.0), Some(1));
        assert_eq!(lyrics.current_index(1e9), Some(2));
    }

    #[test]
    fn loads_sidecar_next_to_audio_file() {
        let dir = std::env::temp_dir().join(format!("replayer-lyrics-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("song.flac");
        std::fs::write(&audio, b"not audio").unwrap();
        assert!(Lyrics::load_for(&audio).is_none());
        std::fs::write(dir.join("song.lrc"), "[00:01.00]hello").unwrap();
        let lyrics = Lyrics::load_for(&audio).unwrap();
        assert!(lyrics.synced);
        assert_eq!(lyrics.lines[0].text, "hello");
        // The longer companion name wins only when the sidecar is absent.
        std::fs::remove_file(dir.join("song.lrc")).unwrap();
        assert!(Lyrics::load_for(&audio).is_none());
        std::fs::write(dir.join("song.flac.lrc"), "[00:02.00]world").unwrap();
        assert_eq!(Lyrics::load_for(&audio).unwrap().lines[0].text, "world");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
