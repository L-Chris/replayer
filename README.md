# Pordo

A media app for watching videos, listening to music and reading comics, with optional desktop AI subtitles and comic translation.

## Features

- **Video and music in one place.** Open local files, manage queues and favorites, and return to recent media. Music shows cover art, track details and available lyrics.
- **A comfortable comic reader.** Connect to [pordo-service](https://github.com/PordoTech/pordo-service), search its enabled sources and build a bookshelf, check for new chapters and resume where you stopped. Choose scrolling or page-by-page reading in either direction.
- **AI subtitles.** Generate Chinese or English subtitles while watching, optionally show both languages, and export the results as SRT.
- **Comic translation.** Translate a page or chapter with a compatible image-editing service, then switch between the original, translation and side-by-side views.
- **Online music.** Search supported music sources, choose a quality, and listen while downloading or save tracks for later.
- **Magnet playback.** Choose a file from a magnet link and start watching or listening as it downloads.
- **Sync between devices.** Use your own WebDAV storage for supported settings, favorites and progress, with optional encrypted API-key sync.
- **Chinese and English desktop interfaces**, full-screen viewing and controls that stay out of the way.

Online sources and AI features depend on their respective services. AI services require compatible models and may charge for requests.

## Download and start

Download a package from [Releases](https://github.com/PordoTech/pordo/releases).

Windows builds through v0.5.1 validate updates against the former repository path. When the first newer version is available, download and install that update manually once from this repository; subsequent updates use the new address.

| Platform | Package |
| --- | --- |
| Windows x64 | Installer, or a portable ZIP to extract and run |
| macOS 15+ · Apple Silicon | `macos-aarch64` DMG or ZIP |
| macOS 15+ · Intel | `macos-x86_64` DMG or ZIP |
| Android 9+ · arm64 | `android-arm64-v8a.apk` for music, comics and WebDAV sync |

On macOS, drag `Pordo.app` into Applications. The app includes its playback dependencies. Downloaded builds are currently ad-hoc signed; macOS may ask you to allow the app in **System Settings → Privacy & Security**.

**Platform notes:** Windows uses Credential Manager and supports installing updates from Settings → About. macOS uses Keychain; download updates from Releases. Android stores credentials with Android KeyStore protection and uses a persistent release signing key for APK updates. See [Android setup and development](docs/android.md) for supported features and signing details.

Open a file or drop it onto the window to begin. Use **F11** for full screen and **Esc** to leave it. During media playback, **Space** pauses/resumes, the arrow keys seek and **M** toggles mute.

See the [user guide](docs/guide.md) for AI setup, music sources, comic reading, sync and current limitations.

## Development

Built with Rust, egui/WGPU and CPAL. Windows and macOS source builds use [remdia](https://github.com/PordoTech/remdia) component graphs for playback and subtitle audio, without FFmpeg. Playback, comic image processing and network work run outside the UI's main rendering path. See the [remdia integration](docs/remdia.md) for build steps and current codec limits; existing published releases may use the earlier backend.

See [building and testing](https://github.com/PordoTech/pordo/blob/main/docs/development.md), [architecture](https://github.com/PordoTech/pordo/blob/main/docs/architecture.md) and [release instructions](https://github.com/PordoTech/pordo/blob/main/docs/releasing.md). Component credits and licenses are listed in [third-party notices](docs/third-party.md).

Originally developed with Qwen3.8 Max.
