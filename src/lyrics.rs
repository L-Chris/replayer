//! Timed LRC lyrics for the music view. Shared parsing and rendering live in
//! the core; the desktop adds cue-based generation from the subtitle pipeline.
pub use replayer_core::lyrics::*;

pub fn from_cues(cues: &[crate::subtitles::Cue]) -> Lyrics {
    Lyrics::from_timed(
        cues.iter()
            .map(|cue| LyricLine {
                time: cue.start,
                text: cue.text.clone(),
            })
            .collect(),
    )
}
