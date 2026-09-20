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
