# Bundled components

Cloud sync XML parsing uses `quick-xml` 0.42.0 (MIT), bundled in
`licenses/quick-xml-MIT.txt`. Source: https://github.com/tafia/quick-xml .
Credential encryption, password-based key derivation and SHA-256 hashing use
`aws-lc-rs` 1.18.1 / `aws-lc-sys` 0.45.0, sharing the implementation already used
by rustls and librqbit. Their notices are bundled as
`licenses/aws-lc-rs-LICENSE.txt` and `licenses/aws-lc-sys-LICENSE.txt`.
Sources: https://github.com/aws/aws-lc-rs and https://github.com/aws/aws-lc .
The application no longer directly depends on ring or sha2; supported Windows
and macOS dependency graphs do not enable ring. Cargo.lock can still contain
ring as an optional dependency of upstream packages.

The v1 sync envelope retains PBKDF2-HMAC-SHA256 with 600,000 iterations,
16-byte salt, AES-256-GCM with a 12-byte nonce, identity as authenticated data,
and ciphertext followed by its 16-byte tag. SHA-256 strings remain lowercase
hexadecimal, preserving cache keys, download filenames and installer digests.

GUI rendering uses WGPU with the DX12 backend on Windows and Metal on macOS.
Vulkan, GLES and web backends are not enabled in these desktop configurations.
Accessibility, default fonts, links, clipboard support and the macOS window
resize jitter fix remain enabled.

UI icons use a compile-time subset of Phosphor Regular through `egui-phosphor`
0.14.0 (MIT OR Apache-2.0). The Phosphor icon artwork is MIT licensed.
Both MIT notices are bundled in `licenses/egui-phosphor-MIT.txt` and
`licenses/Phosphor-Icons-MIT.txt`.
Sources: https://github.com/amPerl/egui-phosphor and
https://github.com/phosphor-icons/web .

Comic images, artwork and translation pixel operations use the remdia system image pack (WIC / ImageIO / CoreGraphics). The application has no direct image or fast_image_resize dependency. The GUI framework still brings its internal image/clipboard dependencies transitively. WebP decoding requires an installed system codec; no third-party WebP fallback is bundled.

Comic library storage uses `rusqlite` 0.40.2 (MIT), which embeds SQLite through
`libsqlite3-sys` with the bundled feature. SQLite is in the public domain.
The rusqlite license is bundled in `licenses/rusqlite-MIT.txt`.
Sources: https://github.com/rusqlite/rusqlite and https://sqlite.org/copyright.html .

Windows and macOS source builds statically register remdia's utility, platform
media and system image factories (Media Foundation / VideoToolbox / WIC / ImageIO).
macOS LAME remains a separately loaded extension. The executable and extension do not link
to FFmpeg. `Cargo.toml` pins the SDK source revision; `scripts/setup-windows.ps1`
and `scripts/setup-macos.sh` build that revision and packaging includes its `REVISION`, `THIRD_PARTY.md`
and license texts. Published releases built before this migration retain their
original bundled notices.

The native utility pack has no third-party media algorithm implementation. Separate
system packs use Media Foundation / WIC on Windows and AVFoundation,
AudioToolbox, VideoToolbox, ImageIO and CoreGraphics on macOS. These operating
system binaries are not redistributed. Codec availability depends on the installed
system components.

The compatibility demux uses Symphonia (MPL-2.0) for unsupported
containers and progressive HTTP sources. The separate macOS extension uses
LAME / mp3lame-encoder (LGPL) for MP3 export. Windows uses the system MP3 encoder with a separate owned gapless
mux. Software Vorbis/WebP decoders are not bundled; those formats require an
installed system decoder. System availability is queried before selecting the relevant graph node.
Symphonia's upstream ADTS/MPA/FLAC reader crates also contain codec code, but the
compatibility demux does not register or call those decoders. Rubato and libopus
are no longer included in the SDK. AVC/HEVC parameter parsing uses h264-reader
and scuffle-h265 (MIT OR Apache-2.0); video decoding uses the system framework.

- https://github.com/L-Chris/remdia
- https://github.com/L-Chris/remdia/blob/main/THIRD_PARTY.md
- https://github.com/pdeljanov/Symphonia
- https://lame.sourceforge.io/

The SDK notices record the exact matching source, dependency versions and rebuild
instructions. macOS `Contents/Resources/remdia` records the SDK revision, source
archive URL, licenses and rebuild/replacement commands. Component libraries stay
separate and replaceable; modified app bundles must be signed again. Builds and
integration tests do not require FFmpeg.

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
