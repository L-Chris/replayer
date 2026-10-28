# remdia integration

Windows and macOS use the public [remdia SDK](https://github.com/L-Chris/remdia),
pinned to an exact Git revision in Cargo.toml. The core loads independent media
components through C ABI v2.1. Neither the executable nor its media plugins links
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
audio and video decoders. Audio passes through a sinc resample/mix component to
CPAL. Windows Media Foundation and macOS VideoToolbox return retained NV12 planes
to the WGPU renderer. Thumbnail requests add an RGBA conversion/resize filter.
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
Local audio and timestamped containers retain precise seek paths. Unsupported
codecs return a clear error rather than using another decoder backend.

## Validation and packaging

After setup, set `REPLAYER_NATIVE_FIXTURES` to the SDK's `tests/fixtures`:

```bash
cargo test --locked
cargo test --locked native_media -- --ignored --nocapture
cargo clippy --locked --all-targets -- -D warnings
```

On macOS, source the generated environment before standalone Cargo commands.
CI builds and tests native Apple Silicon and Intel packages plus Windows x64.
Packaging explicitly includes both plugins, manifests, matching revision, notices
and codec rebuild instructions. Relocated app tests remove development runtime
paths and use only the system PATH. See [packaging](releasing.md).
