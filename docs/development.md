# Development

Run commands from the repository root. Use the fixed Cargo preview executable; do not create a new executable name for each change.

## Windows

Install Rust, Visual Studio C++ Build Tools/Windows SDK and LLVM with `libclang.dll`.

```powershell
./scripts/setup-windows.ps1 -WithTestTools
./scripts/preview.ps1
./scripts/preview.ps1 -Media "path/to/song.flac"
./scripts/preview.ps1 -NoLaunch
```

Setup provides verified FFmpeg headers/libraries and configures `FFMPEG_DIR`, `LIBCLANG_PATH` and the runtime PATH for the current shell. Without a custom runtime archive it uses the original pinned full runtime for development. This fallback cannot be used to create release packages.

For the optimized runtime, download the `windows-ffmpeg-runtime` CI artifact, keeping its ZIP and checksum manifest together under `target/windows-runtime`, then run:

```powershell
./scripts/setup-windows.ps1 -RuntimeArchive target/windows-runtime/ffmpeg-playback.zip -WithTestTools
```

`-WithTestTools` supplies a separate full FFmpeg and sets `REPLAYER_TEST_FFMPEG` for fixture generation. Its libraries are not added as the app's runtime. `preview.ps1` rebuilds/restarts only this checkout's `target/debug/replayer.exe`; a failed build never launches an older executable.

## macOS

Requires macOS 15+, Rust, Xcode Command Line Tools and Homebrew build tools. Intel builds require NASM.

```bash
brew install pkg-config nasm
./scripts/build-macos.sh
./target/debug/replayer "path/to/video.mkv"
```

The first build compiles checksum-pinned FFmpeg 9.0.2 and LAME 4.0 into `.deps/macos-aarch64` or `.deps/macos-x86_64`. Playback does not depend on Homebrew libraries. Before standalone Cargo commands, source the generated environment:

```bash
source .deps/macos-aarch64/env.sh  # use macos-x86_64 on Intel
```

Fixture generation needs a separate full tool:

```bash
brew install ffmpeg
export REPLAYER_TEST_FFMPEG="$(brew --prefix ffmpeg)/bin/ffmpeg"
```

The app's runtime remains the pinned `.deps` build. Do not use the Windows paths in `.cargo/config.toml` for native macOS builds.

## Checks

With the platform environment and fixture tool configured:

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Use `REPLAYER_SETTINGS_FILE` pointing inside `target/` to isolate developer settings. Ordinary tests use generated media and local mock services, and do not consume model credits. Tests find the full fixture executable through `REPLAYER_TEST_FFMPEG`, or on PATH outside `FFMPEG_DIR`.

Additional checks are explicit:

```text
cargo test --locked gpu_yuv_readback -- --ignored --nocapture
cargo test --locked comic_catalog_visuals -- --ignored --nocapture
cargo test --locked subtitles
cargo test --locked comics
cargo test --locked music_sources
```

GPU/visual tests require an adapter. Visual artifacts are written under `target/`. Live source/model tests are ignored by default and may use the network or incur service charges. Their test-only helper reads `.env.example`-documented values from an explicit `.env` or `REPLAYER_ENV_FILE`; the app, preview and subtitle CLI never read those files.

On Windows, `./verify.ps1` generates playback fixtures with `REPLAYER_TEST_FFMPEG`; `-Software` forces software decoding and `-Stress` adds a larger case. CLI smoke tests are `replayer --selftest <video>` and `replayer --selftest-music <audio>`; the music check uses the actual output device. Keep all fixtures/logs inside `target/`.

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

Release Rust builds use LTO, one codegen unit and stripped symbols. Debug previews keep their symbols. HTTP clients share reqwest 0.13 and the AWS-LC rustls provider; the separate `ring` dependency remains for cloud-sync encryption.

Both FFmpeg builds source `scripts/ffmpeg-playback-options.sh`. All playback decoders, demuxers, protocols and hardware acceleration from the platform baseline remain available; only the MP3 encoder used by subtitle uploads is enabled. Test encoders and pure video-encoding libraries are excluded. Both packagers inspect the actual runtime's encoder list and reject extra encoders.

Windows uses `scripts/build-windows-runtime.sh` with a digest-pinned BtbN LGPL build image and checksum-pinned FFmpeg source. Run it on Linux/macOS with Docker and Python 3. It compares the optimized configuration against the full baseline and fails on lost playback capabilities. The ZIP and checksum manifest are written to `target/windows-runtime/`; CI caches them by recipe hashes. Windows setup verifies the manifest and refreshes extraction when its archive changes.

macOS builds its shared runtime locally and includes matching source archives and the public recipe. No test tool, Homebrew runtime tree, environment dump or user data is packaged. See [releasing](releasing.md) for package commands and [architecture](architecture.md) for implementation boundaries.
