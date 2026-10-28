# remdia integration

Windows and macOS use the public [remdia SDK](https://github.com/L-Chris/remdia),
pinned to an exact Git revision in Cargo.toml. The core loads independent media
components through C ABI v2.2. Neither the executable nor its media plugins links
FFmpeg. The `remdia-media` feature remains as a compatibility alias; remdia is
now the default backend.

## Build and preview

Windows requires Rust stable, Visual Studio C++ Build Tools, CMake and Python 3:

```powershell
./scripts/setup-windows.ps1
./scripts/preview.ps1 -Media 'D:\media\movie.mp4'
```

macOS 15+ requires Rust stable, CMake and Apple command-line tools:

```bash
./scripts/build-macos.sh
source .deps/macos-aarch64/env.sh  # macos-x86_64 on Intel
./target/debug/replayer /path/to/movie.mp4
```

Setup builds the pinned SDK into `.deps/remdia-source`, validates its revision,
architecture and ABI, then stages matching plugins beside the fixed preview
executable. `REMDIA_RUNTIME_DIR` can select a prebuilt matching SDK runtime;
`REMDIA_PLUGIN_DIR` overrides plugin discovery during development. macOS bundles
load their plugins from `Contents/Frameworks/remdia`.

## Media graphs

Playback routes compressed packets from the demux component to independent
audio and video decoders. Audio passes through a system resample/mix component to
CPAL. Windows Media Foundation and macOS VideoToolbox return retained NV12 planes
to the WGPU renderer. Thumbnail requests connect the video conversion node to image adaptation/scaling nodes.
Subtitle extraction requests 16 kHz mono PCM; a separate LAME encoder produces
MP3 upload chunks. Graphs stay on their worker; immutable frames cross threads.

The host retains asynchronous open, pause, seek generations, bounded queues,
progressive HTTP buffering, cancellation, EOF/replay and audio-device recovery.
Tests read a committed synthetic corpus and require no external media encoder.

The SDK accepts arbitrary compatible demux/decoder/filter/encoder/mux/sink DAGs,
including fanout and compressed-packet paths that bypass decoding. The replayer
adapter uses the playback and subtitle graphs rather than exposing an editor.

## Current limits

H.264 in MP4/MKV is the primary video verification path. Windows currently uses
synchronous software MFT decoding. macOS supports VideoToolbox H.264/HEVC,
progressive 8-bit 4:2:0 SDR; H.264 is covered by real-file tests. macOS automatic
mode prefers hardware and falls back to system software; `REPLAYER_SOFTWARE=1`
requests software. Frames are copied to CPU NV12 buffers; Metal texture sharing,
Main10/HDR and asynchronous decode ABI extensions remain future work. In-band
codec parameter changes require reopening from a keyframe.

Remote elementary MPEG audio uses approximate byte/duration seeking to avoid
scanning a missing download prefix; VBR accuracy is limited by this estimate.
Local audio and timestamped containers retain precise seek paths. Windows MF
FLAC currently pre-rolls from the beginning because its native nonzero seeks
return premature EOF for valid small-block streams; long-file seek latency can
grow with the target position. Unsupported
codecs return a clear error rather than using another decoder backend.

Audio decoding and resampling now use Media Foundation / AudioToolbox for MP3,
AAC, FLAC, ALAC, PCM and Opus; macOS also supplies Vorbis. Dolby availability
depends on the installed system components. Windows Vorbis has a separate narrow
fallback. Complete local files use the independent system demux; progressive
HTTP/Range and unsupported containers use the compatibility demux and the same
system decoders. The 16 kHz gapless subtitle MP3 contract retains a separate LAME
fallback. No libopus or Rubato is bundled. See the pinned SDK's capability report
for the installed machine; unsupported codecs fail explicitly.

## Validation and packaging

After setup, set `REPLAYER_NATIVE_FIXTURES` to the SDK's `tests/fixtures`:

```bash
cargo test --locked
cargo test --locked native_media -- --ignored --nocapture
cargo clippy --locked --all-targets -- -D warnings
```

On macOS, source the generated environment before standalone Cargo commands.
CI builds and tests native Apple Silicon and Intel packages plus Windows x64.
Packaging explicitly includes seven macOS packs or nine Windows packs, their manifests, matching revision, notices
and codec rebuild instructions. Relocated app tests remove development runtime
paths and use only the system PATH. See [packaging](releasing.md).

## Image graphs

`src/pixels.rs` submits `MediaRequest::Image` jobs to the same SDK Graph used by playback. The application no longer directly depends on image/fast_image_resize or a local image codec crate. `Image` is a read-only view over the shared MediaBuffer; UI texture uploads consume its dimensions/stride/bytes. Comic previews use decode -> tile, while translation uses crop/scale/compose/alpha/encode nodes. JPEG uploads and caches retain the white-background quality-95 policy. Existing PNG/WebP/JPEG caches remain readable.

Both packages include the system image pack. Windows also includes a separate WebP fallback, selected only when WIC cannot activate and decode a known WebP capability fixture; tagged WebP ICC/EXIF currently needs a system codec. macOS uses ImageIO for WebP decoding. The GUI framework still has its own transitive image/clipboard dependencies; removing the application codecs does not claim those crates disappear from the whole Cargo graph.
