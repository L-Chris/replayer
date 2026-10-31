# User guide

## Install and update

Get the package for your platform from [Releases](https://github.com/PordoTech/pordo/releases). Windows offers an installer and portable ZIP. macOS 15+ offers separate Apple Silicon and Intel DMG/ZIP packages; move `Pordo.app` to Applications. When upgrading from `replayer.app`, quit the old app and remove its app bundle after installing Pordo; existing settings are kept.

Windows updates can be checked, downloaded and installed from **Settings → About**. On macOS, use the Releases link. Export generated subtitles before restarting.

Service credentials are saved through Windows Credential Manager or macOS Keychain; Android protects its local credentials with Android KeyStore. Local playback and comic reading do not require an AI account.

The repository moved to `PordoTech/pordo`. Windows clients through v0.5.1 still validate installer URLs against the previous repository. Install the first newer version manually from Releases once it is available; later clients use the new location.

## Music and video

Open one or more files or drop them onto the window. **Add Files** or Shift-drop appends files; a normal open/drop replaces the current queue. Music and video queues are saved independently. Removing an item from a queue does not delete its file.

The app chooses music or video mode from the media tracks. Music shows available title, artist, album, cover art and lyrics. Embedded tags take priority; downloaded-track metadata fills gaps. A matching `.lrc` file can supply timed lyrics.

Click the **Lyrics** icon in the playback bar, then choose **Generate AI lyrics** to transcribe a local song or a fully downloaded online track. **Show lyrics** opens the lyric view. Generation uses the audio-capable model selected under **AI Subtitles**, keeps the song's original language, and displays completed lines while generating. On successful completion, lyrics are automatically saved as a matching `.lrc` file beside the song and loaded next time. **Regenerate lyrics** replaces that file after the new generation succeeds. Cancelling, failed segments, or no recognized lyrics leave the saved file unchanged. Existing lyrics remain visible until new lines are recognized. Instrumental passages can produce no lyrics; recognition and timing depend on the selected model. Generating lyrics sends the song's audio chunks to the configured AI service.

Use the queue sidebar to select, remove or reorder entries. Hold an item's title before dragging to reorder. Favorites and recent activity are available from the media pages and Home. **Play all** on music favorites uses the displayed favorites, including online tracks not downloaded on this device.

| Shortcut | Action |
| --- | --- |
| Space | Pause/resume media |
| Left / Right | Seek during media playback; turn pages in the comic reader |
| M | Mute/unmute media |
| F11 | Toggle full screen |
| Esc | Leave full screen before other back actions |
| F / double-click | Toggle full screen during video playback |

Video and comic toolbars appear when the pointer approaches the edges. Music controls stay visible.

### Online music

Configure the music-source API key in **Settings → Music**. The ChKSz adapter exposes NetEase Cloud Music, QQ Music and Kugou search and playback; availability and quality depend on the upstream service.

**Play** starts playback while downloading when the source permits it. **Download** saves a complete file. Existing downloads are reused; unavailable quality is reported rather than silently replaced. The download directory is configurable, and completed files remain there after playback.

Keep `.pordo.json`, `.pordo-cover.png` (or their older `.replayer` equivalents) and any `.lrc` sidecars with their audio file when moving downloaded music. They contain metadata, cover art and lyrics that may not be embedded in the audio.

### QQ Music local files

Some QMC/MFLAC/MGG variants are supported. If the player requests a song key, use **Import song key and retry**, or put its Base64 ekey beside the media as `song.mflac.ekey`, retaining the original extension. This is a per-song key; the player does not retrieve account credentials or keys from other applications.

Legacy QMC1 and supported QMC2 Map/RC4 variants can be opened. Some metadata-only variants require an external ekey. EncV2 and unknown footer versions remain unsupported; the extension alone does not establish compatibility. Finish downloading encrypted torrent files before opening them locally.

### Magnet links

Choose **Open magnet link**, wait for metadata and select a supported file. Playback waits for verified downloaded data; seeking prioritizes the requested location. Poor peer availability or a proxy blocking BT traffic can delay playback.

The task menu supports file selection, pause/resume and stop. Pausing the download also pauses playback. Stopping retains downloaded files; torrent sessions do not continue after the app exits. Cache files are not automatically removed. Pure v2 magnets without a v1 `btih` hash are not supported.

AI subtitles can use audio already present in the torrent cache. Missing audio may delay generation. For TUN/proxy setups, use the routing guidance offered by the magnet dialog; keep any AI-service proxy rule ahead of a process-wide direct rule.

## Comics

Configure the service base URL and device Token in **Settings → Comics** (Android: **Comics → Source settings**). Create the device Token and configure upstream accounts in the [pordo-service](https://github.com/PordoTech/pordo-service) management page. Search its enabled sources, open a title and choose a chapter. Sources remain separate even when titles match. Add books to your bookshelf, check updates and resume from history. Komiic chapters can be filtered by chapter/volume type.

Choose vertical scrolling, left-to-right or right-to-left pages. Zoom, page position and reading progress are saved. The chapter button opens the chapter list; the page slider and previous/next controls navigate within the book. A new favorite starts with the existing chapter list as its baseline; later unread additions produce update indicators.

The device Token stays in this device’s OS credential vault and is excluded from WebDAV sync. Existing source passwords are no longer read or sent; configure those accounts on the service. Original pages and covers are cached. Clearing these caches in Settings preserves translations, favorites and history. Previously cached pages may remain readable when a source is unavailable or its image quota is exhausted. Some upstream sources require adapter updates or are unsupported.

### Translate comic pages

Add a compatible service in **Settings → AI Providers**, then select an image-editing model in the comic settings. The service must accept an image-edit request and return an edited image; a text-only model is insufficient.

Choose **Translate page** or **Translate chapter**. Original, translated and side-by-side views are available. Original images are preserved, and cached translations can be reused. Old PNG/WebP translations remain usable; new translated results use JPEG. A failed page does not remove its original or prevent later pages from continuing.

Translation is started explicitly. Cancelling stops local waiting, but an already accepted request may still be billed. Generated images can change artwork or mistranslate text; compare with the original.

## AI subtitles

In **Settings → AI Providers**, enter a compatible service address, key and available models. Fetch the model list or add an ID manually. Select an audio-capable model under **AI Subtitles**; comic translation selects its model separately.

Open a video and choose **Subtitles → Generate AI subtitles**. Completed segments appear during playback. The subtitle menu can hide, cancel or export the results. Export needed results as SRT before closing or switching media: generated subtitles are not automatically saved.

The interface language selects Chinese or English output. Optional bilingual subtitles show the original and target language. Generation prioritizes the current playback position. MP3 audio is the default, with WAV available for compatible services; model support varies between providers.

Only explicitly requested audio segments are sent to the selected AI service, not video frames or the local file path. Timing and wording are model estimates, and failed segments may leave gaps. Check the result before relying on it. Cancelling a request does not guarantee cancellation of server-side charges.

## Cloud sync and local data

Configure an HTTPS WebDAV service in **Settings → Cloud Sync**, test the connection, then enable sync. Use the same account on another device. Offline changes stay local and are merged after reconnecting.

Sync covers supported service/model settings, comic and online-music favorites, reading/listening progress and related preferences. It does not upload local media files or paths, playback queues, downloaded audio, covers, comic pages, translated images or subtitle caches.

API keys are excluded by default. To sync them, enable the option and use the same separate sync passphrase on all devices; keep the original passphrase available when changing it. WebDAV passwords and the passphrase itself are not uploaded. A wrong passphrase stops writes rather than replacing existing encrypted keys.

Windows data normally lives in `%LOCALAPPDATA%/pordo`. macOS defaults to `~/.config/pordo`; a configured `XDG_CONFIG_HOME` changes that location. Existing profiles in the corresponding `replayer` directory are reused automatically. Downloads and caches can occupy significant space. Stop a torrent before manually deleting its cache, and retain downloaded-music sidecars when moving files.

The app does not read development `.env` files. Provider keys belong in the app's supported credential settings. See [third-party notices](third-party.md) for bundled components and [Releases](https://github.com/PordoTech/pordo/releases) for changes.
