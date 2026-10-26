# Desktop builds and updates

The desktop workflow builds and tests every main-branch push, pull request and
manual dispatch. Its jobs run natively on Windows x64, macOS 15 Apple Silicon
(`macos-15`) and macOS 15 Intel (`macos-15-intel`). No API key is needed to build.
Windows produces an installer and portable ZIP; each macOS architecture produces
a ZIP containing `replayer.app` and a drag-to-install DMG. Workflow artifacts
include platform-specific SHA-256 manifests.

To release:

1. Update the version in `Cargo.toml` and regenerate `Cargo.lock`.
2. Add bilingual notes at `docs/releases/vX.Y.Z.md`, validate them with
   `python scripts/validate-release-notes.py --tag vX.Y.Z --file docs/releases/vX.Y.Z.md`,
   then commit and push a matching stable tag. Follow the release notes rules in `AGENTS.md`.
3. The workflow rejects mismatched tags, runs lint and tests, and builds packages.
   It tests the installed Windows app and the relocated app extracted from both
   macOS package formats, including dependency paths, signatures and playback.
4. Only after all three platform builds pass does it create the GitHub Release and upload assets.
   The release title is the tag. If a release already exists, it uploads the
   packages and preserves the existing notes. A branch push never publishes a release.

The publish job combines all platform manifests into `SHA256SUMS.txt`, verifies
every package, and uploads both the combined manifest and the platform manifests.
All release assets share the Cargo version. The macOS packages are named
`replayer-VERSION-macos-aarch64.{zip,dmg}` and
`replayer-VERSION-macos-x86_64.{zip,dmg}`; these are separate native builds, not
a universal binary.

## macOS packaging

Install Xcode Command Line Tools, Rust and `brew install pkg-config nasm`, then:

```bash
./scripts/build-macos.sh --release
source .deps/macos-aarch64/env.sh  # use macos-x86_64 on Intel
python3 scripts/package-macos.py
python3 scripts/test-macos-package.py
```

The full smoke test mounts the DMG read-only, which can prompt for diskutil access
on a local Mac. Use `--skip-dmg-mount` to verify the DMG checksum and test the ZIP
app without mounting. CI always performs the full mounted-image test.

`setup-macos.sh` pins FFmpeg 9.0.2 and LAME 4.0 source archives by SHA-256 and
builds shared libraries without GPL/nonfree components or Homebrew runtime
dependencies. The FFmpeg CLI is a build/test tool; it is not copied into the app.
The app includes only the executable's transitive dylib dependency closure,
third-party notices, matching source archives and the public build recipe.
It never includes configure logs, build environment dumps, user media or settings.
Mach-O install names are rewritten relative to the app, and external non-system
dependencies or mismatched architectures/deployment targets fail packaging.
The minimum supported macOS is 15.0 on both architectures.

Packages are ad-hoc signed by default. This verifies bundle integrity and supports
local execution; it does not provide Developer ID trust or Apple notarization.
Users of downloaded builds may need to approve opening the app in System Settings
→ Privacy & Security. For distribution with an existing local Developer ID identity
and a configured notarytool keychain profile:

```bash
export MACOS_CODESIGN_IDENTITY='Developer ID Application: Your Name (TEAMID)'
export MACOS_NOTARY_PROFILE='replayer-notary'
python3 scripts/package-macos.py
```

Libraries are signed individually before the app, with the hardened runtime when
a Developer ID identity is supplied. The app and disk image are submitted and
stapled before final checksums are calculated. No certificates or credentials are
created or stored by these scripts, and CI uses ad-hoc signing unless a separate
signing environment is provisioned. macOS updates currently use the releases
link in About; automatic installer-based updates remain Windows-only.

## Windows packaging and updates

Windows packages include FFmpeg shared libraries and statically link the Rust
application's MSVC runtime.

Update discovery uses GitHub's latest stable release API. Each installer must be
named `replayer-VERSION-windows-x86_64-setup.exe` and have a GitHub SHA-256 asset
digest. Missing hashes, untrusted URLs, invalid sizes and non-newer versions are
rejected. There are no signing secrets to configure. These packages are not yet
Authenticode-signed; SHA-256 verification does not replace publisher signing.

Settings → About displays the current version, project and releases links,
automatic-check preference, check/download progress, release notes and the
install/restart action. Automatic checks run once on startup in release builds;
debug builds check only when requested. No update is silently installed.

The installer is per-user (no elevation by default) and installs to
`%LOCALAPPDATA%/Programs/replayer`. Updates reuse the currently running executable's
directory, including a writable portable directory. The helper waits for the
player to exit, verifies the hash again, installs, and restarts the player. Settings
in `%LOCALAPPDATA%/replayer` and credentials in Windows Credential Manager are not
included in packages or overwritten. The app does not read `.env`; it is reserved
for explicit development tests.
Export generated subtitles before restarting. Installation logs are under
`%LOCALAPPDATA%/replayer/updates/VERSION/`.

Release notes can follow Torto's bilingual format:

```markdown
## Feature

- English release notes.

<details>
<summary>中文更新说明</summary>

## Feature

- 中文更新说明。

</details>
```

The updater selects the notes matching the interface language. When no translation
is available, it shows the full notes.

For a local package, install Inno Setup 6, build the release binary and run:

```powershell
./scripts/setup-windows.ps1
cargo build --locked --release
./scripts/package-windows.ps1
```

Use `-SkipInstaller` to create only the portable ZIP. Dependency URLs and checksums
are pinned in `scripts/setup-windows.ps1`. Update them together when upgrading FFmpeg.

The verified FFmpeg 9.0.2 archive is mirrored in the `deps-ffmpeg-9.0.2`
prerelease because upstream daily archives expire. This dependency release is
excluded from stable application update discovery. Its bytes and SHA-256 match
the original pinned upstream archive; do not replace it with a different build.
