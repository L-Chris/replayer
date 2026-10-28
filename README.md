# replayer

A desktop app for watching videos, listening to music and reading comics, with optional AI subtitles and comic translation.

## Features

- **Video and music in one place.** Open local files, manage queues and favorites, and return to recent media. Music shows cover art, track details and available lyrics.
- **A comfortable comic reader.** Search MangaYun and Komiic, build a bookshelf, check for new chapters and resume where you stopped. Choose scrolling or page-by-page reading in either direction.
- **AI subtitles.** Generate Chinese or English subtitles while watching, optionally show both languages, and export the results as SRT.
- **Comic translation.** Translate a page or chapter with a compatible image-editing service, then switch between the original, translation and side-by-side views.
- **Online music.** Search supported music sources, choose a quality, and listen while downloading or save tracks for later.
- **Magnet playback.** Choose a file from a magnet link and start watching or listening as it downloads.
- **Sync between devices.** Use your own WebDAV storage for supported settings, favorites and progress, with optional encrypted API-key sync.
- **Chinese and English interfaces**, full-screen viewing and controls that stay out of the way.

Online sources and AI features depend on their respective services. AI services require compatible models and may charge for requests.

## Download and start

Download a package from [Releases](https://github.com/L-Chris/replayer/releases).

| Platform | Package |
| --- | --- |
| Windows x64 | Installer, or a portable ZIP to extract and run |
| macOS 15+ · Apple Silicon | `macos-aarch64` DMG or ZIP |
| macOS 15+ · Intel | `macos-x86_64` DMG or ZIP |

On macOS, drag `replayer.app` into Applications. The app includes its playback dependencies. Downloaded builds are currently ad-hoc signed; macOS may ask you to allow the app in **System Settings → Privacy & Security**.

**Platform notes:** Windows supports saved service credentials and installing updates from Settings → About. macOS credential storage is not implemented yet, which limits features requiring saved API keys or passwords; macOS updates are downloaded from Releases.

Open a file or drop it onto the window to begin. Use **F11** for full screen and **Esc** to leave it. During media playback, **Space** pauses/resumes, the arrow keys seek and **M** toggles mute.

See the [user guide](docs/guide.md) for AI setup, music sources, comic reading, sync and current limitations.

## Development

Built with Rust, egui/WGPU and CPAL. Windows and macOS source builds use [remdia](https://github.com/L-Chris/remdia) component graphs for playback and subtitle audio, without FFmpeg. Playback, comic image processing and network work run outside the UI's main rendering path. See the [remdia integration](docs/remdia.md) for build steps and current codec limits; existing published releases may use the earlier backend.

See [building and testing](https://github.com/L-Chris/replayer/blob/main/docs/development.md), [architecture](https://github.com/L-Chris/replayer/blob/main/docs/architecture.md) and [release instructions](https://github.com/L-Chris/replayer/blob/main/docs/releasing.md). Component credits and licenses are listed in [third-party notices](docs/third-party.md).

Originally developed with Qwen3.8 Max.
