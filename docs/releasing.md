# Releases and packaging

## CI and versioning

The desktop workflow builds Windows x64, macOS Apple Silicon and macOS Intel. Branch/PR builds upload artifacts; a stable `vX.Y.Z` tag matching `Cargo.toml` publishes only after all platform jobs pass.

Windows FFmpeg is cross-built on Linux using the pinned dependency image before the Windows app job. macOS builds natively on each architecture. CI runs formatting, Clippy, tests, release builds and package smoke tests. The Windows test installs the package and plays media with developer runtime paths removed; the macOS test checks signatures, relocation and playback from ZIP and mounted DMG.

To release:

1. Update `Cargo.toml` and `Cargo.lock`.
2. Write `docs/releases/vX.Y.Z.md` following [the notes format](releases/README.md) and [repository rules](../AGENTS.md).
3. Run `python3 scripts/validate-release-notes.py --tag vX.Y.Z --file docs/releases/vX.Y.Z.md`.
4. Commit the intended changes and push the matching tag when ready to publish.

The release title must equal the tag. Notes are English first, followed by Simplified Chinese inside `<details>` with `<summary>中文更新说明</summary>`. Use Feature, Improvement and Fix in that order, omitting empty groups. Rebuilding an existing tag preserves its notes. The publish job verifies package hashes, publishes the supplied notes, then verifies the fetched title/body.

## macOS packages

Configure the [build environment](development.md#macos), then:

```bash
./scripts/build-macos.sh --release
source .deps/macos-aarch64/env.sh  # use macos-x86_64 on Intel
python3 scripts/package-macos.py
export REPLAYER_TEST_FFMPEG="$(brew --prefix ffmpeg)/bin/ffmpeg"
python3 scripts/test-macos-package.py
```

This creates `replayer-VERSION-macos-ARCH.zip`, `.dmg` and a SHA-256 manifest in `dist/`. Use `--output-directory` on the packager and `--directory` on the tester for a review folder under `target/`.

The ZIP contains `replayer.app`; the DMG provides drag-to-install. Packages include only the executable's transitive dylib closure, icons, user documentation, notices and matching FFmpeg/LAME source archives/recipes. Install names are rewritten relative to the app; unexpected non-system dependencies, architecture mismatches and incompatible deployment targets fail validation. Both architectures require macOS 15+.

The full smoke test mounts the DMG read-only. If local disk permissions prevent mounting, `--skip-dmg-mount` verifies image integrity and tests the relocated ZIP app. CI always mounts the DMG.

### Signing

Default packages are ad-hoc signed, without Developer ID notarization. With an existing signing identity and configured notarytool profile:

```bash
export MACOS_CODESIGN_IDENTITY='Developer ID Application: Your Name (TEAMID)'
export MACOS_NOTARY_PROFILE='replayer-notary'
python3 scripts/package-macos.py
```

Libraries are signed before the app. Developer ID builds use the hardened runtime, submit/staple the app and image, and calculate final checksums afterward. The scripts do not create credentials or certificates.

## Windows packages

Install Inno Setup 6 and prepare the optimized runtime as described in [development](development.md#package-size-policy). Keep `ffmpeg-playback.zip` with its `SHA256SUMS.txt` under `target/windows-runtime`:

```powershell
./scripts/setup-windows.ps1 -RuntimeArchive target/windows-runtime/ffmpeg-playback.zip -WithTestTools
cargo build --locked --release
./scripts/package-windows.ps1
```

CI passes its explicit target binary to the packager. Local builds use `target/release/replayer.exe` by default. `-SkipInstaller` produces only the portable ZIP. The full legacy runtime remains a development/test bootstrap; packaging rejects it because it exposes extra encoders.

Output names are `replayer-VERSION-windows-x86_64-setup.exe` and `.zip`, with a SHA-256 manifest. The app includes five FFmpeg DLLs (`avcodec`, `avformat`, `avutil`, `swresample`, `swscale`); import-table checks reject excluded FFmpeg dependencies. Runtime provenance and public recipes accompany the notices. CI statically links the app's MSVC runtime.

The installer is per-user, defaults to `%LOCALAPPDATA%/Programs/replayer` and does not replace user settings or downloaded media. Installed-update smoke tests also check shortcut/icon behavior.

## Update behavior

Windows update discovery uses GitHub's latest stable release. A newer installer must have the expected versioned filename, trusted URL, valid size and GitHub SHA-256 asset digest. The helper verifies the hash again, waits for the app to exit, installs to the current application directory and restarts it. Checks do not silently install an update. Logs are in the user's `replayer/updates/VERSION/` directory.

The packages are not currently Authenticode-signed; hashes are integrity checks, not publisher signatures. macOS currently links users to Releases rather than installing updates in-app.

The old full Windows FFmpeg archive is mirrored in the `deps-ffmpeg-9.0.2` prerelease for development/test bootstrap and is excluded from application-update discovery. Its URL/checksum stay paired in `scripts/setup-windows.ps1`; optimized builds are pinned by `scripts/build-windows-runtime.sh`. Component attribution is in [third-party notices](third-party.md).
