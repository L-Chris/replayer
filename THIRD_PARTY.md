# Bundled components

Cloud sync XML parsing uses `quick-xml` 0.42.0 (MIT), bundled in
`licenses/quick-xml-MIT.txt`. Source: https://github.com/tafia/quick-xml .
Credential encryption and password-based key derivation use `ring` 0.17.14;
its applicable notices are bundled as `licenses/ring-LICENSE.txt`,
`licenses/ring-BoringSSL.txt` and `licenses/ring-other-bits.txt`.
Source: https://github.com/briansmith/ring .

UI icons use a compile-time subset of Phosphor Regular through `egui-phosphor`
0.14.0 (MIT OR Apache-2.0). The Phosphor icon artwork is MIT licensed.
Both MIT notices are bundled in `licenses/egui-phosphor-MIT.txt` and
`licenses/Phosphor-Icons-MIT.txt`.
Sources: https://github.com/amPerl/egui-phosphor and
https://github.com/phosphor-icons/web .

Comic preview resizing uses `fast_image_resize` 6.1.0 (MIT OR Apache-2.0),
Copyright Kirill Kuzminykh. The MIT license is bundled in
`licenses/fast_image_resize-MIT.txt`.
Source: https://github.com/Cykooz/fast_image_resize .

Comic library storage uses `rusqlite` 0.40.2 (MIT), which embeds SQLite through
`libsqlite3-sys` with the bundled feature. SQLite is in the public domain.
The rusqlite license is bundled in `licenses/rusqlite-MIT.txt`.
Sources: https://github.com/rusqlite/rusqlite and https://sqlite.org/copyright.html .

Windows packages include FFmpeg shared libraries from the BtbN LGPL shared build.
The FFmpeg license is included as `FFmpeg-LICENSE.txt`; the libraries remain
separate DLLs and can be replaced with ABI-compatible builds.
Packages include only `avcodec`, `avformat`, `avutil`, `swresample` and `swscale`.
Device capture (`avdevice`) and filter graphs (`avfilter`) are not used or bundled;
packaging checks the executable and DLL import tables for excluded FFmpeg dependencies.

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
