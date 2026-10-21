# Bundled components

Comic library storage uses `rusqlite` 0.40.2 (MIT), which embeds SQLite through
`libsqlite3-sys` with the bundled feature. SQLite is in the public domain.
The rusqlite license is bundled in `licenses/rusqlite-MIT.txt`.
Sources: https://github.com/rusqlite/rusqlite and https://sqlite.org/copyright.html .

Windows packages include FFmpeg shared libraries from the BtbN LGPL shared build.
The FFmpeg license is included as `FFmpeg-LICENSE.txt`; the libraries remain
separate DLLs and can be replaced with ABI-compatible builds.

CI dependency source, build scripts and configuration:

- https://github.com/BtbN/FFmpeg-Builds
- https://github.com/L-Chris/replayer/releases/tag/deps-ffmpeg-9.0.2
- https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
- https://ffmpeg.org/legal.html

The pinned archive and SHA-256 are recorded in `scripts/setup-windows.ps1`.
The dependency release mirrors the unmodified, checksum-verified BtbN archive
from `autobuild-2026-09-19-13-11`; upstream daily releases expire after 14 days.
See the upstream build repository for bundled codec libraries, notices and source
retrieval instructions. Replayer does not modify the FFmpeg libraries.

Magnet playback embeds `librqbit` 9.0.1 and its rqbit support crates, Copyright
2021 Igor Katson, licensed under Apache-2.0. The license is bundled in
`licenses/Apache-2.0.txt`. Source: https://github.com/ikatson/rqbit/tree/v9.0.1

The QQ Music adapter includes Rust adaptations of the QMC1 mask from
https://github.com/Presburger/qmc-decoder (Copyright 2019 Presburger) and QMC2
ciphers/key envelopes from https://github.com/nukemiko/libtakiyasha
(Copyright 2023 nukemiko). Both MIT notices are bundled in
`licenses/QQ-Music-adapters.txt`. No authentication/credential extraction code
is included. Tencent TEA uses `tc_tea` 0.2.1 (MIT OR Apache-2.0), source:
https://github.com/jixunmoe/tc_tea_rust .
