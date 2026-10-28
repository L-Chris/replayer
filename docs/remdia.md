# Windows remdia integration

Windows uses the public [remdia SDK](https://github.com/L-Chris/remdia), pinned
to an exact Git revision in Cargo.toml. Its core loads independent components
through C ABI v2. Neither the Windows executable nor its media DLLs depends on
FFmpeg. macOS retains the existing backend while Windows integration is validated.

## Build and preview

Install Rust stable, Visual Studio C++ Build Tools, CMake and Python 3. Run:

```powershell
./scripts/setup-windows.ps1
./scripts/preview.ps1 -Media 'D:\media\movie.mp4'
```

Setup fetches the pinned SDK into `.deps/remdia-source`, builds its two component
DLLs, and verifies their revision and ABI. Preview copies them beside the fixed
`target/debug/replayer.exe`. A previously staged runtime can be supplied through
`-RuntimeDirectory` or `REMDIA_RUNTIME_DIR`; its REVISION must match Cargo.toml.
The application normally discovers `plugins/` beside its executable.
`REMDIA_PLUGIN_DIR` overrides that directory for development.

## Media graphs

Playback routes compressed packets from the demux component to independent
audio and video decoders. Audio passes through a sinc resample/mix component to
CPAL; Windows video reaches the WGPU renderer as retained NV12 planes. Thumbnail
requests add an RGBA conversion/resize filter. Subtitle extraction requests
16 kHz mono PCM; a separate LAME encoder produces MP3 upload chunks. Graphs and
components stay on their owning worker; immutable frames can cross threads.

The host retains asynchronous open, pause, seek generations, bounded queues,
progressive HTTP buffering, cancellation, EOF/replay and audio-device recovery.
Native tests use a committed synthetic corpus, without invoking a media tool.
The portable `remdia-media` Cargo feature runs this audio integration on macOS
or Linux for development; Windows selects it automatically.

## Current limits

H.264 in MP4/MKV is the primary Windows video verification path. Other codecs
depend on the demuxer's recognised parameters and installed Media Foundation
transforms. Video currently uses synchronous software decoding; hardware MFT,
D3D surface sharing and HDR tone mapping are not implemented. The migration
does not promise parity with every codec or container in older releases.

The SDK accepts arbitrary compatible demux/decoder/filter/encoder/mux/sink DAGs,
including fanout and compressed-packet paths that bypass decoding. The replayer
adapter uses the playback and subtitle graphs rather than exposing an editor.

## Validation and packaging

```powershell
$env:REPLAYER_NATIVE_FIXTURES = "$PWD/.deps/remdia-source/tests/fixtures"
cargo test --locked native_media -- --ignored --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
./scripts/package-windows.ps1
```

CI builds the exact SDK revision, exercises native audio and video, checks
Windows Cargo dependencies and PE imports, and runs the installed application's
selftest with only the Windows system PATH. Packaging bundles both component
DLLs, manifests and matching notices. The installer removes DLL names shipped
by the previous Windows backend. No release is published by a feature build.
