# Architecture

## Module boundaries

| Area | Responsibility |
| --- | --- |
| `src/app/` | UI, navigation, settings drafts and reader/media views |
| `src/player/` | Media sessions, demux, decoding, audio output and playback clock |
| `src/renderer/native.rs`, `src/video.wgsl` | GPU video conversion and presentation |
| `src/media*`, `src/queue.rs`, `src/lyrics.rs` | Metadata, local adapters, saved queues and lyrics |
| `src/music_sources/` | Source adapters, downloads and streaming cache |
| `src/comics/`, `src/pixels.rs` | Comic sources, library, image preparation and translation |
| `src/subtitles/` | Audio extraction, AI scheduling, subtitle validation and SRT |
| `src/sync/`, `src/credentials.rs` | WebDAV merge protocol, encryption and credential storage |
| `src/torrent.rs`, `src/updater.rs` | Torrent bridge and release discovery/installation |

## Playback

The UI holds an asynchronous `Player` handle. A worker owns a remdia demux/decoder/filter graph; a session coordinates bounded frame queues, audio output and presentation. Codec implementations live in independent component plugins behind C ABI v2.2. The audio callback uses a preallocated SPSC PCM buffer and atomic control/progress data, avoiding locks and allocation in the callback.

The audio device drives the clock when audio is available; underruns freeze progress at submitted audio. A wall clock allows silent video and video beyond a short audio track to continue. EOF drains decoders, resampling, device output and remaining video before reporting completion. Ended sessions retain enough state for seeking and replay.

Each seek increments an epoch. Old audio and video are rejected, packet queues are flushed and the demux worker seeks. Video before the requested position is discarded and audio is clipped to the target. Completion means the new position is prepared, not that the device has already played it. Stop interrupts blocking I/O; worker reclamation stays off the UI thread.

Windows uses software Media Foundation transforms. macOS uses synchronous VideoToolbox decoding with hardware preference and system software fallback. Both return CPU NV12 planes to WGPU; thumbnails use a separate conversion/resize component. The current verified path is 8-bit SDR H.264 in MP4/MKV. Shared GPU textures and HDR tone mapping remain future work. See [media integration](remdia.md) for supported paths and limits.

## Network media

Online music uses a shared disk cache and a token-protected loopback HTTP bridge. Range-capable sources support prioritized reads for seeking; other sources may need sequential download. Complete files are published without overwriting an existing destination, so the download directory must support hard links. Provider API credentials are kept away from media/CDN requests.

Torrent reads are gated by verified pieces, not sparse-file length. Playback requests prioritize pieces; subtitle extraction uses a separate cache-only endpoint. Task URLs use random tokens on ephemeral loopback ports, and no public torrent-management API is exposed.

QQ local adapters prepare supported encrypted audio in bounded, cancellable work without modifying the source or transcoding it. Temporary decrypted playback files are cleaned up on normal close; crashes can leave files behind.

## Comics and images

`ComicSource` isolates search, details, chapters and page requests. Stable identities include provider/site and upstream work identity; equal titles are not merged. SQLite stores the bookshelf, chapter state and reading progress.

Original downloads are cached unchanged and detected by content. JPEG/WebP/PNG request preferences do not trigger lossy conversion of ordinary original pages. DM5 can fall back to its public reader data when aggregation fails; remote scripts are parsed for data and are not executed. Komiic requests use short-lived tickets, bounded concurrency and quota backoff; tickets and ticketed URLs are not persisted.

Image decoding, resizing, tiling, cropping, composition and encoding use remdia image components in the same graph runtime as audio/video. Windows uses WIC and macOS uses ImageIO/CoreGraphics; a narrow Windows WebP plugin covers systems without a WIC WebP decoder. Processing runs in bounded background work. Prepared images use memory budgets and shared in-flight requests. Source-byte changes and explicit disk-cache clearing invalidate the corresponding memory entries.

Image translation uses an independent model configuration and `images/edits` adapter. Uploads and new translated results use JPEG at quality 95, with transparent areas composited on white. Long pages are split with overlap and restored to their canvas dimensions. Originals are preserved, old PNG/WebP translations remain valid, and cache identity includes source content, service/model, language and generation options. Encoding is bounded and performed off the UI thread. A failed page is not automatically charged again by an unconditional retry.

## Subtitles

An independent input decodes audio to 16 kHz mono PCM and encodes MP3 with delay/padding metadata, or writes WAV. Work is bounded by chunk/request queues and prioritizes the current playback position. Results retain original media offsets and are merged in time order.

The service returns validated timed segments; bilingual output adds original text. HTTP 400 is skipped immediately, 429/5xx use bounded retry/backoff, and failed segments are reported while later work continues. Cancellation and seek preemption are distinct from failed segments.

Formatting keeps at most two centered lines, preserves dialogue and language-appropriate punctuation, and splits long text without truncation. Timing adjustments are bounded by neighboring cues/chunk limits. Reading-speed warnings are diagnostics, not transcription accuracy measurements or forced alignment. The same normalized text is shown and exported to SRT.

## Persistence and sync

Settings, media queues and comic data are separate. Secrets are not written into settings JSON; the current system vault backend supports Windows only. Runtime paths use the application data root, with `PORDO_SETTINGS_FILE` available for isolated tests.

WebDAV sync exports a whitelist into per-device documents under `replayer/v1/devices/`. Version vectors and deletion markers merge changes; concurrent unfavorite wins, explicit later refavorite remains possible, and progress can move backward on a deliberate reread. Each device writes its own document using ETag conditions and persists unacknowledged local changes.

Optional API-key sync uses PBKDF2-HMAC-SHA256 (600,000 iterations) and AES-256-GCM with fresh salt/nonce and authenticated record identity. Devices without a passphrase preserve opaque ciphertext; decryption errors stop writes. WebDAV credentials and the passphrase never enter the sync payload.

See [development](development.md) for validation and package-size policy, and the [user guide](guide.md) for supported workflows.
