//! Platform-neutral replayer logic shared by the desktop app and the Android
//! bridge: comic/music sources, bookshelf library, caches, downloads, lyrics
//! and remdia-backed media jobs. Platform integration (UI, audio output, OS
//! vaults, loopback streaming) stays in each application crate.
pub mod comics;
pub mod crypto;
pub mod jobs;
pub mod lyrics;
pub mod media;
pub mod media_backend;
pub mod music_sources;
pub mod paths;
pub mod pixels;
pub mod sync;
