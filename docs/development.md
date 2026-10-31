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
`preview.ps1` rebuilds/restarts only this checkout's `target/debug/pordo.exe`;
a failed build never launches an older executable.

## macOS

Requires macOS 15+, Rust stable, Apple command-line tools and CMake.

```bash
brew install cmake
./scripts/build-macos.sh
./target/debug/pordo "path/to/video.mkv"
```

The first build fetches and builds the pinned remdia SDK. Built-in components
are linked into pordo; only the LAME extension is copied beside the fixed
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

Use `PORDO_SETTINGS_FILE` pointing inside `target/` to isolate developer settings. Ordinary tests use committed synthetic media and local mock services, and do not consume model credits. Set `PORDO_NATIVE_FIXTURES` to `.deps/remdia-source/tests/fixtures` and run `cargo test --locked native_media -- --ignored --nocapture` for the real plugin tests.

Additional checks are explicit:

```text
cargo test --locked gpu_yuv_readback -- --ignored --nocapture
cargo test --locked comic_catalog_visuals -- --ignored --nocapture
cargo test --locked subtitles
cargo test --locked comics
cargo test --locked music_sources
```

GPU/visual tests require an adapter. Visual artifacts are written under `target/`. Live source/model tests are ignored by default and may use the network or incur service charges. Their test-only helper reads `.env.example`-documented values from an explicit `.env` or `PORDO_ENV_FILE`; the app, preview and subtitle CLI never read those files.

On Windows, `./verify.ps1` reuses the fixed preview and committed MP4/MKV corpus; `-Software` forces software decoding and `-Stress` repeats the seek/drain regressions 25 times. CLI smoke tests are `pordo --selftest <video>` and `pordo --selftest-music <audio>`; the music check uses the actual output device. Keep all fixtures/logs inside `target/`.

## Diagnostics and CLI

- `PORDO_SOFTWARE=1`: force software video decoding.
- `PORDO_STATS=1`: playback diagnostics; Windows preview also accepts `-Stats`.
- `PORDO_SETTINGS_FILE`: isolate settings and related application data.
- `PORDO_TORRENT_CACHE`: override the torrent cache root.

Subtitle CLI examples:

```text
pordo --subtitles input.mkv output.srt --language zh-CN --concurrency 3
pordo --subtitles input.mkv sample.srt --language en-US --start 600 --duration 60
```

The CLI uses saved provider/model settings, preserves absolute media timestamps and refuses to overwrite an existing SRT. Language/concurrency arguments do not modify the saved UI configuration.

## Package-size policy

Release Rust builds use LTO, one codegen unit and stripped symbols. Debug previews keep their symbols. HTTP clients, cloud-sync encryption and SHA-256 hashing share AWS-LC; the application no longer directly depends on `ring` or `sha2`. WGPU enables only DX12 on Windows and Metal on macOS, preserving accessibility, fonts, clipboard support and the macOS window resize fix.

Bundled SQLite keeps core SQL, transactions and WAL. `.cargo/config.toml` disables its unused FTS3/FTS5, R-tree and dbstat extensions through `LIBSQLITE3_FLAGS` for both desktop platforms. The comic library uses ordinary tables; no database migration or data-file rewrite is required. A macOS ARM64 comparison reduced machine code by about 0.42 MiB. Environment overrides of `LIBSQLITE3_FLAGS` can change these build options.

Both platforms statically register remdia utility, system media/image and
compatibility demux factories in pordo. Windows needs no SDK DLLs; macOS
keeps only the separate LAME extension. The factories still compose independent
graph nodes and external extensions retain ABI 2.2. Windows MFT and macOS
VideoToolbox supply video decoding. macOS copies CPU NV12 planes to WGPU;
Metal texture sharing and HDR remain future work.

Packagers verify architecture, ABI, revision and native imports and reject FFmpeg
runtime files. No test tool, Homebrew runtime tree, environment dump or user data
is packaged. See [releasing](releasing.md) for package commands and
[architecture](architecture.md) for implementation boundaries.

## Brand and upgrade compatibility

The application is Pordo; Cargo packages, executables and new download sidecars use
`pordo`. Existing desktop profiles under `replayer` are reused without moving them;
new profiles use `pordo`. `PORDO_*` developer overrides also accept their old
`REPLAYER_*` aliases, with the new name taking precedence.

Published identities intentionally remain stable: Android application ID
`dev.replayer` (source namespace `dev.pordo`), its old launcher component alias,
macOS bundle ID `io.github.l-chris.replayer`, the Windows installer AppId,
OS credential namespaces and Android keystore alias/AAD. The WebDAV v1 path
`replayer/v1/devices/` and encrypted formats remain compatible across devices.
Existing `.replayer.json` / `.replayer-cover.png` downloads remain readable;
new writes use `.pordo.json` / `.pordo-cover.png`.

Historical release notes, design references and measurement records retain their
original filenames and product names. Existing release tags are immutable;
release backfills derive artifact/JNI names from the tagged source checkout.

Branding validation on macOS ARM64 (2026-10-31): 159 desktop/core tests,
strict Clippy, 8 Python tool tests and workflow lint passed. Android API 28
passed 21 offline cases (3 network-gated skips), 13 release APK audit checks,
and upgrades from both legacy debug and production-signed APKs. The debug
upgrade retained files, preferences and keystore-encrypted secrets; both
upgrades retained UID and first-install time. The legacy launcher alias opened
the renamed app. macOS ZIP relocation, signing and playback passed; DMG
checksum verification passed but mounting was unavailable on this host.
Windows installer migration checks are included in CI and were not run locally.

## Content service integration

Comics require [pordo-service](https://github.com/PordoTech/pordo-service). Configure
its root URL and a per-device Token in Settings → Comics (Android: Comics → Source
settings). Source credentials, login, tickets, quotas and parsing live on the server.
The client preserves original provider/site/detail URL and chapter ID/URL identities,
local library/read/progress records, page cache keys and the WebDAV v1 protocol.
Device tokens are excluded from serialized settings and cloud sync; old source
passwords remain untouched in the vault for rollback and are no longer read.

The client uses source-site discovery and original chapter URLs from the service;
use the companion service contract PR when testing this branch. Metadata and
image requests never follow redirects with device credentials. Original-byte
caches are checked before resolving an asset ID, so a cached image needs no request.

For repeatable integration without public sources, start the opt-in fixture from
`pordo-service/server` (Node 24; build the management SPA first):

```sh
TMPDIR=/absolute/path/to/replayer/target/service-integration/ node --import tsx \
  tests/client-fixture.ts /absolute/path/to/replayer/target/service-integration/fixture.json \
  /absolute/path/to/remdia/tests/fixtures/cover.png
```

It creates an isolated real API server with a signed MangaYun mock, valid PNGs,
admin `admin` / `test-password-123`, and a private fixture file containing only a
temporary device token. It changes neither the normal service database nor accounts.
Use the printed local URL to test the management UI search → detail → pages flow.
Run the desktop client contract from the Pordo checkout after sourcing the native env:

```sh
PORDO_SERVICE_FIXTURE_FILE="$PWD/target/service-integration/fixture.json" \
  cargo test --locked -p pordo-core content_service_end_to_end -- --ignored --nocapture
```

For an isolated Android debug device, reverse the fixture's TCP port with adb and
copy that JSON into `files/content-service-fixture.json` using `adb shell -T run-as
 dev.replayer tee ...` with host-file stdin and suppressed stdout. Run
`ContentServiceTest#realServiceReadsLegacyFavoritesDirectoriesAndImages` through the
normal instrumentation runner. Without this explicit fixture it is assumption-skipped.
Stop the fixture process when done; it removes its temporary DB/cache and token file.

Validation on macOS ARM64 and Android API 28: source discovery, legacy resolve,
original chapter URLs, page image decode, persisted favorite/read/progress, revoked
Tokens, credential isolation, redirect rejection and cached-byte reuse passed.
The management SPA's search/details/page-image flow and settings rendering at 628/
1030 px passed. Windows runtime verification remains for CI.

A local same-profile artifact comparison against fd6ce5e (same remdia pin, no version
bump) measured the raw ARM64 macOS release executable at 25,144,608 → 24,946,064 B
(−0.79%) and the Gradle ARM64 Android release APK at 11,482,126 → 10,005,718 B
(−12.86%). These are neither Windows installer nor installed-footprint measurements.
Warm incremental release rebuilds took 92 s (macOS) and 26.6 s (Android, Cargo +
Gradle); there is no comparable baseline time, so no build-speed claim is made.
The isolated test service sampled at 53.1 MiB RSS after requests; a 120-byte cached
PNG fixture is not a representative public-source workload. Raw measurements/logs
stay under target/service-integration and are not committed.
