# Windows builds and updates

The Windows workflow builds and tests every main-branch push, pull request and
manual dispatch. It uploads a Windows x64 installer, a portable ZIP and
`SHA256SUMS.txt` as workflow artifacts. It includes FFmpeg shared libraries and
links the Rust application's MSVC runtime statically. No API key is needed to build.

To release:

1. Update the version in `Cargo.toml` and regenerate `Cargo.lock`.
2. Commit, then create and push a matching stable tag such as `v0.2.0`.
3. The workflow rejects mismatched tags, runs tests, builds packages, installs the
   installer in the disposable runner, and runs the installed player's self-test.
4. Only after the build passes does it create the GitHub Release and upload assets.
   The release title is the tag. A branch push never publishes a release.

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
in `%LOCALAPPDATA%/replayer` and the user's `.env` are not included or overwritten.
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
