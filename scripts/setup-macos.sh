#!/usr/bin/env bash
# Build a small, pinned shared FFmpeg runtime rather than shipping Homebrew's tree.
set -euo pipefail
[[ $(uname -s) == Darwin ]] || { echo 'This script requires macOS.' >&2; exit 1; }
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
case $(uname -m) in
  arm64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) echo 'Unsupported macOS architecture.' >&2; exit 1 ;;
esac
for tool in clang make curl pkg-config python3; do
  command -v "$tool" >/dev/null || { echo "Missing $tool (install Xcode Command Line Tools and brew install pkg-config)." >&2; exit 1; }
done
if [[ $arch == x86_64 ]] && ! command -v nasm >/dev/null; then
  echo 'Intel builds require nasm: brew install nasm' >&2
  exit 1
fi
prefix="$root/.deps/macos-$arch"
sources="$root/.deps/macos-sources"
work="$root/target/macos-dependencies-$arch"
minimum=${MACOSX_DEPLOYMENT_TARGET:-15.0}
clang_path=$(xcrun --find clang)
clang_lib="$(dirname "$(dirname "$clang_path")")/lib"
[[ -f $clang_lib/libclang.dylib ]] || { echo "Missing libclang in $clang_lib" >&2; exit 1; }
mkdir -p "$prefix/share/replayer" "$sources" "$work"
build_id=$( { shasum -a 256 "${BASH_SOURCE[0]}"; printf '%s\n' "$arch" "$minimum" "$prefix"; } | shasum -a 256 | cut -d ' ' -f 1)

download() {
  local filename=$1 expected=$2 url=$3
  if [[ ! -f $sources/$filename ]] || [[ $(shasum -a 256 "$sources/$filename" | cut -d ' ' -f 1) != "$expected" ]]; then
    curl --fail --location --retry 3 "$url" --output "$sources/$filename.part"
    [[ $(shasum -a 256 "$sources/$filename.part" | cut -d ' ' -f 1) == "$expected" ]] || { echo "Checksum mismatch: $filename" >&2; exit 1; }
    mv "$sources/$filename.part" "$sources/$filename"
  fi
}

if [[ ! -f $prefix/share/replayer/build-id.txt ]] || [[ $(cat "$prefix/share/replayer/build-id.txt") != "$build_id" ]]; then
  download ffmpeg-9.0.2.tar.xz 8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e \
    https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
  download lame-4.0.tar.gz 3df5124d5ad3a98312ffd7ba6a9b36230e4f8a3e66d3ce0f425e336c32d216eb \
    https://downloads.sourceforge.net/project/lame/lame/4.0/lame-4.0.tar.gz
  # Only disposable dependency build trees in this checkout are replaced.
  rm -rf "$work/ffmpeg-9.0.2" "$work/lame-4.0"
  tar -xf "$sources/ffmpeg-9.0.2.tar.xz" -C "$work"
  tar -xf "$sources/lame-4.0.tar.gz" -C "$work"
  jobs=$(sysctl -n hw.logicalcpu)
  export MACOSX_DEPLOYMENT_TARGET="$minimum"
  (
    cd "$work/lame-4.0"
    # The upstream ID3 code still uses legacy declarations (also handled by Homebrew).
    ac_cv_prog_cc_c23=no CFLAGS="-O2 -mmacosx-version-min=$minimum -Wno-implicit-function-declaration" \
      ./configure --prefix="$prefix" --build="$arch-apple-darwin" \
      --disable-static --enable-shared --disable-frontend --disable-decoder --disable-dependency-tracking
    make -j "$jobs"
    make install
  )
  (
    cd "$work/ffmpeg-9.0.2"
    PKG_CONFIG_PATH="$prefix/lib/pkgconfig" ./configure \
      --prefix="$prefix" --arch="$arch" --cpu=generic --cc=clang \
      --enable-shared --disable-static --disable-debug --disable-doc \
      --disable-autodetect --disable-gpl --disable-nonfree --disable-version3 \
      --disable-ffplay --disable-ffprobe --disable-indevs --enable-indev=lavfi --disable-outdevs \
      --enable-libmp3lame --enable-securetransport --enable-zlib --enable-bzlib --enable-iconv \
      --enable-videotoolbox --enable-audiotoolbox \
      --extra-libs=-liconv \
      --extra-cflags="-I$prefix/include -mmacosx-version-min=$minimum" \
      --extra-ldflags="-L$prefix/lib -mmacosx-version-min=$minimum"
    make -j "$jobs"
    make install
  )
  rm -rf "$prefix/share/replayer/sources"
  mkdir -p "$prefix/share/replayer/licenses" "$prefix/share/replayer/sources"
  cp "$work/ffmpeg-9.0.2/COPYING.LGPLv2.1" "$prefix/share/replayer/licenses/FFmpeg-LGPL-2.1.txt"
  cp "$work/lame-4.0/COPYING" "$prefix/share/replayer/licenses/LAME-LGPL.txt"
  # Configure logs can dump the build environment. Ship only the public recipe.
  python3 - "$prefix" "$arch" "$minimum" "$work/ffmpeg-9.0.2/ffbuild/config.mak" <<'PY'
from pathlib import Path
import sys
prefix, arch, minimum, config = sys.argv[1:]
flags = next(line.split('=', 1)[1] for line in Path(config).read_text().splitlines()
             if line.startswith('FFMPEG_CONFIGURATION='))
(Path(prefix) / 'share/replayer/sources/BUILD.txt').write_text(
    'FFmpeg 9.0.2 + LAME 4.0, built from the included unmodified source archives.\n'
    + 'Architecture: ' + arch + '\nMinimum macOS: ' + minimum + '\n'
    + 'FFmpeg configure: ' + flags.replace(prefix, '<PREFIX>') + '\n'
    + 'The complete build recipe and source SHA-256 values are in setup-macos.sh.\n', encoding='utf-8')
PY
  cp "$sources/ffmpeg-9.0.2.tar.xz" "$sources/lame-4.0.tar.gz" "$prefix/share/replayer/sources/"
  cp "$root/scripts/setup-macos.sh" "$prefix/share/replayer/sources/"
  printf '%s\n' "$build_id" > "$prefix/share/replayer/build-id.txt"
fi

"$prefix/bin/ffmpeg" -hide_banner -version
python3 - "$prefix" "$clang_lib" "$minimum" <<'PY'
import os, shlex, sys
from pathlib import Path
prefix, clang, minimum = sys.argv[1:]
values = {'FFMPEG_DIR': prefix, 'LIBCLANG_PATH': clang,
          'PKG_CONFIG_PATH': prefix + '/lib/pkgconfig', 'MACOSX_DEPLOYMENT_TARGET': minimum}
env = Path(prefix) / 'env.sh'
env.write_text(''.join('export ' + key + '=' + shlex.quote(value) + '\n' for key, value in values.items())
               + 'export PATH=' + shlex.quote(prefix + '/bin') + ':"$PATH"\n', encoding='utf-8')
if os.environ.get('GITHUB_ENV'):
    with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as file:
        file.writelines(key + '=' + value + '\n' for key, value in values.items())
    with open(os.environ['GITHUB_PATH'], 'a', encoding='utf-8') as file:
        file.write(prefix + '/bin\n')
print('Build environment: source ' + shlex.quote(str(env)))
PY
