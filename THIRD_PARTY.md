# Bundled components

Windows packages include FFmpeg shared libraries from the BtbN LGPL shared build.
The FFmpeg license is included as `FFmpeg-LICENSE.txt`; the libraries remain
separate DLLs and can be replaced with ABI-compatible builds.

CI dependency source, build scripts and configuration:

- https://github.com/BtbN/FFmpeg-Builds/tree/autobuild-2026-09-19-13-11
- https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-09-19-13-11
- https://ffmpeg.org/legal.html

The pinned archive and SHA-256 are recorded in `scripts/setup-windows.ps1`.
See the upstream build repository for bundled codec libraries, notices and source
retrieval instructions. Replayer does not modify the FFmpeg libraries.
