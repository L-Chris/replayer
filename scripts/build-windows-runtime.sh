#!/usr/bin/env bash
# Cross-build with BtbN's complete LGPL dependency set, retaining its decoders.
# Run on Linux with Docker (also works with Docker Desktop on macOS).
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
image='ghcr.io/btbn/ffmpeg-builds/win64-lgpl-shared-9.0@sha256:80f61efc8daac808aa14bdeefa63274d946af4c71cca0ef133a0864460259877'
if [[ ${1:-} == --inside ]]; then
  source "$root/scripts/ffmpeg-playback-options.sh"
  work="$root/target/windows-runtime-build"
  prefix="$work/prefix"
  mkdir -p "$work"
  rm -rf "$prefix"
  cd "$work/ffmpeg-9.0.2"
  # These variables are supplied by the digest-pinned BtbN build image.
  # Intentional word splitting: upstream stores compiler/configure flags as strings.
  configure_runtime() {
    ./configure --prefix="$prefix" --pkg-config-flags=--static \
    $FFBUILD_TARGET_FLAGS $FF_CONFIGURE \
    --disable-doc "$@" \
    --extra-cflags="$FF_CFLAGS" --extra-cxxflags="$FF_CXXFLAGS" \
    --extra-libs="$FF_LIBS" --extra-ldflags="$FF_LDFLAGS" \
    --extra-ldexeflags="$FF_LDEXEFLAGS" \
    --cc="$CC" --cxx="$CXX" --ar="$AR" --ranlib="$RANLIB" --nm="$NM"
  }
  configure_runtime
  cp config_components.h "$work/baseline-components.h"
  configure_runtime "${playback_options[@]}"
  python3 "$root/scripts/verify-ffmpeg-config.py" config_components.h --baseline "$work/baseline-components.h"
  make -j "$(nproc)"
  make install
  mkdir -p "$prefix/lib"
  cp "$prefix"/bin/*.lib "$prefix/lib/"
  cp COPYING.LGPLv3 "$prefix/LICENSE.txt"
  cp "$root/scripts/build-windows-runtime.sh" "$root/scripts/ffmpeg-playback-options.sh" \
    "$root/scripts/verify-ffmpeg-config.py" "$prefix/"
  # Only public configure arguments, never config.log or environment dumps.
  grep '^FFMPEG_CONFIGURATION=' ffbuild/config.mak > "$prefix/BUILD.txt"
  printf '\nBuild image: %s\nSource: https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz\nSource SHA256: 8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e\n' "$image" >> "$prefix/BUILD.txt"
  exit
fi
cd "$root"
mkdir -p .deps/macos-sources target
archive=.deps/macos-sources/ffmpeg-9.0.2.tar.xz
expected=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
verify_source() {
  python3 - "$archive" "$expected" <<'PY'
import hashlib, pathlib, sys
p = pathlib.Path(sys.argv[1])
sys.exit(0 if p.is_file() and hashlib.sha256(p.read_bytes()).hexdigest() == sys.argv[2] else 1)
PY
}
if ! verify_source; then
  curl --fail --location --retry 3 https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz --output "$archive.part"
  mv "$archive.part" "$archive"
fi
verify_source || { echo 'FFmpeg source checksum mismatch.' >&2; exit 1; }
mkdir -p target/windows-runtime-build
rm -rf target/windows-runtime-build/ffmpeg-9.0.2
tar -xf "$archive" -C target/windows-runtime-build
docker run --rm --platform linux/amd64 --user "$(id -u):$(id -g)" \
  --volume "$root:/workspace" --workdir /workspace "$image" \
  bash /workspace/scripts/build-windows-runtime.sh --inside
python3 - <<'PY'
import hashlib
from pathlib import Path
import zipfile
prefix = Path('target/windows-runtime-build/prefix')
output = Path('target/windows-runtime')
output.mkdir(exist_ok=True)
archive = output / 'ffmpeg-playback.zip'
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for p in sorted(prefix.rglob('*')):
        if p.is_file():
            z.write(p, str(p.relative_to(prefix)))
(output / 'SHA256SUMS.txt').write_text(hashlib.sha256(archive.read_bytes()).hexdigest()
                                     + '  ' + archive.name + '\n', encoding='ascii')
print('Windows runtime:', archive, archive.stat().st_size, 'bytes')
PY
