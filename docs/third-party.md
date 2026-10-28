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

Windows source builds use remdia's independently loaded native and Windows
Media Foundation component DLLs. The executable and component packs do not link
to FFmpeg. `Cargo.toml` pins the SDK source revision; `scripts/setup-windows.ps1`
builds that revision and packaging includes its `REVISION`, `THIRD_PARTY.md`
and license texts. Published releases built before this migration retain their
original bundled notices.

The native pack contains Symphonia (MPL-2.0), libopus (BSD-3-Clause), Rubato
(MIT), and LAME / mp3lame-encoder (LGPL). The LGPL encoder is contained in the
replaceable native DLL. The matching source revision, dependencies and rebuild
instructions are recorded in the SDK's third-party notices; its Cargo.lock pins
the codec dependencies. Windows Media Foundation is supplied by the operating
system; codec availability depends on the installed Windows components.

- https://github.com/L-Chris/remdia
- https://github.com/L-Chris/remdia/blob/main/THIRD_PARTY.md
- https://github.com/pdeljanov/Symphonia
- https://opus-codec.org/
- https://lame.sourceforge.io/

macOS packages use FFmpeg 9.0.2 built without GPL/nonfree components and LAME 4.0
as shared libraries. `Contents/Resources/licenses` contains their license texts;
`Contents/Resources/sources` contains the matching unmodified source archives and
`setup-macos.sh`, shared encoding options and the configuration verifier with
the public build recipe and SHA-256 values. Libraries remain
separate and replaceable with ABI-compatible builds (a changed bundle must be
signed again). Sources:

- https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
- https://downloads.sourceforge.net/project/lame/lame/4.0/lame-4.0.tar.gz

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
