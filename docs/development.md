# Development

Run commands from the repository root. Use the fixed Cargo preview executable; do not create a new executable name for each change.

## Windows

Install Rust stable, Visual Studio C++ Build Tools/Windows SDK, CMake and Python 3.

```powershell
./scripts/setup-windows.ps1
./scripts/preview.ps1
./scripts/preview.ps1 -Media "path/to/song.flac"
./scripts/preview.ps1 -NoLaunch
```

Setup builds the exact remdia revision pinned in Cargo.toml. `-RuntimeDirectory`
can select an already staged SDK runtime whose REVISION matches that pin.
`preview.ps1` rebuilds/restarts only this checkout's `target/debug/replayer.exe`;
a failed build never launches an older executable.

## macOS

Requires macOS 15+, Rust stable, Apple command-line tools and CMake.

```bash
brew install cmake
./scripts/build-macos.sh
./target/debug/replayer "path/to/video.mkv"
```

The first build fetches and builds the pinned remdia SDK. Built-in components
are linked into replayer; only the LAME extension is copied beside the fixed
preview executable on macOS. Before
standalone Cargo commands, source the generated environment:

```bash
source .deps/macos-aarch64/env.sh  # use macos-x86_64 on Intel
```

Both platforms use remdia by default, without FFmpeg or an external fixture
generator. See [media integration](remdia.md) for graphs and current codec limits.

## Checks

With the platform runtime configured:

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Use `REPLAYER_SETTINGS_FILE` pointing inside `target/` to isolate developer settings. Ordinary tests use committed synthetic media and local mock services, and do not consume model credits. Set `REPLAYER_NATIVE_FIXTURES` to `.deps/remdia-source/tests/fixtures` and run `cargo test --locked native_media -- --ignored --nocapture` for the real plugin tests.

Additional checks are explicit:

```text
cargo test --locked gpu_yuv_readback -- --ignored --nocapture
cargo test --locked comic_catalog_visuals -- --ignored --nocapture
cargo test --locked subtitles
cargo test --locked comics
cargo test --locked music_sources
```

GPU/visual tests require an adapter. Visual artifacts are written under `target/`. Live source/model tests are ignored by default and may use the network or incur service charges. Their test-only helper reads `.env.example`-documented values from an explicit `.env` or `REPLAYER_ENV_FILE`; the app, preview and subtitle CLI never read those files.

On Windows, `./verify.ps1` reuses the fixed preview and committed MP4/MKV corpus; `-Software` forces software decoding and `-Stress` repeats the seek/drain regressions 25 times. CLI smoke tests are `replayer --selftest <video>` and `replayer --selftest-music <audio>`; the music check uses the actual output device. Keep all fixtures/logs inside `target/`.

## Diagnostics and CLI

- `REPLAYER_SOFTWARE=1`: force software video decoding.
- `REPLAYER_STATS=1`: playback diagnostics; Windows preview also accepts `-Stats`.
- `REPLAYER_SETTINGS_FILE`: isolate settings and related application data.
- `REPLAYER_TORRENT_CACHE`: override the torrent cache root.

Subtitle CLI examples:

```text
replayer --subtitles input.mkv output.srt --language zh-CN --concurrency 3
replayer --subtitles input.mkv sample.srt --language en-US --start 600 --duration 60
```

The CLI uses saved provider/model settings, preserves absolute media timestamps and refuses to overwrite an existing SRT. Language/concurrency arguments do not modify the saved UI configuration.

## Package-size policy

Release Rust builds use LTO, one codegen unit and stripped symbols. Debug previews keep their symbols. HTTP clients, cloud-sync encryption and SHA-256 hashing share AWS-LC; the application no longer directly depends on `ring` or `sha2`. WGPU enables only DX12 on Windows and Metal on macOS, preserving accessibility, fonts, clipboard support and the macOS window resize fix.

Both platforms statically register remdia utility, system media/image and
compatibility demux factories in replayer. Windows needs no SDK DLLs; macOS
keeps only the separate LAME extension. The factories still compose independent
graph nodes and external extensions retain ABI 2.2. Windows MFT and macOS
VideoToolbox supply video decoding. macOS copies CPU NV12 planes to WGPU;
Metal texture sharing and HDR remain future work.

Packagers verify architecture, ABI, revision and native imports and reject FFmpeg
runtime files. No test tool, Homebrew runtime tree, environment dump or user data
is packaged. See [releasing](releasing.md) for package commands and
[architecture](architecture.md) for implementation boundaries.
