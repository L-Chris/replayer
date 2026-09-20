# Development preview

- Reuse the fixed Cargo debug executable; do not make a new executable name for
  every build or feature.
- Use `./scripts/preview.ps1` to rebuild and restart the local preview, or
  `./scripts/preview.ps1 -NoLaunch` when building before Computer Use validation.
- The script closes only `target/debug/replayer.exe` in this checkout (using
  Cargo's actual target directory). Never stop unrelated installed/release apps.
- `-Media "path/to/song.flac"` opens a local media file after rebuilding.
- A failed build must not launch an old executable as though it were current.
