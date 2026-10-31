#!/usr/bin/env bash
# Build the pinned FFmpeg-free SDK, or validate an explicitly supplied runtime.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
target="${REMDIA_TARGET:-$(rustc -vV | awk '/^host:/ {print $2}')}"
case "$target" in
  aarch64-apple-darwin) arch=aarch64 ;;
  x86_64-apple-darwin) arch=x86_64 ;;
  *) echo "Unsupported macOS target: $target" >&2; exit 1 ;;
esac
minimum="${MACOSX_DEPLOYMENT_TARGET:-15.0}"
revision=$(python3 - <<'READ_REV'
import pathlib, re
text = pathlib.Path('Cargo.toml').read_text()
match = re.search(r'^remdia\s*=\s*\{[^}]*rev\s*=\s*"([a-f0-9]{40})"', text, re.M)
assert match, 'Cargo.toml must pin a complete remdia revision'
print(match[1])
READ_REV
)
runtime="${REMDIA_RUNTIME_DIR:-$root/.deps/remdia-source/dist/remdia}"
if [[ ! -f "$runtime/REVISION" ]] || [[ $(tr -d '\r\n' < "$runtime/REVISION") != "$revision" ]]; then
  source_dir="$root/.deps/remdia-source"
  if [[ ! -e "$source_dir/.git" ]]; then
    # A restored build cache may already contain source_dir/target. Initialize
    # around it; a no-checkout clone also has no populated index to audit yet.
    mkdir -p "$source_dir"
    git -C "$source_dir" init
    git -C "$source_dir" remote add origin https://github.com/PordoTech/remdia.git
  else
    git -C "$source_dir" diff --quiet
    git -C "$source_dir" diff --cached --quiet
  fi
  git -C "$source_dir" fetch --depth 1 origin "$revision"
  git -C "$source_dir" checkout --detach "$revision"
  MACOSX_DEPLOYMENT_TARGET="$minimum" "$source_dir/scripts/build-macos.sh" --release --target "$target"
  runtime="$source_dir/dist/remdia"
fi
runtime=$(cd "$runtime" && pwd)
python3 - "$runtime" "$arch" "$revision" <<'VERIFY'
import json, pathlib, subprocess, sys
root, arch, revision = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
assert (root/'REVISION').read_text().strip() == revision
layout=json.loads((root/'BUNDLED.json').read_text())
assert layout['layout']=='unified' and layout['os']=='macos' and layout['arch']==arch
for name in ('fallback_mp3_lame',):
    manifest=json.loads((root/'plugins'/f'{name}.json').read_text())
    assert manifest['abi_major']==2 and manifest.get('abi_minor',0)==2
    assert manifest['os']=='macos' and manifest['arch']==arch
    library=root/'plugins'/manifest['library']
    expected='arm64' if arch=='aarch64' else arch
    assert subprocess.check_output(['lipo','-archs',str(library)],text=True).strip()==expected
VERIFY
mkdir -p "$root/.deps/macos-$arch"
python3 - "$root/.deps/macos-$arch/env.sh" "$runtime" "$minimum" <<'ENV'
import pathlib, shlex, sys
file,runtime,minimum=pathlib.Path(sys.argv[1]),sys.argv[2],sys.argv[3]
file.write_text('export MACOSX_DEPLOYMENT_TARGET='+shlex.quote(minimum)+'\n'
    +'export REMDIA_RUNTIME_DIR='+shlex.quote(runtime)+'\n'
    +'export REMDIA_PLUGIN_DIR='+shlex.quote(runtime+'/plugins')+'\n')
ENV
if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "REMDIA_RUNTIME_DIR=$runtime" >> "$GITHUB_ENV"
  echo "REMDIA_PLUGIN_DIR=$runtime/plugins" >> "$GITHUB_ENV"
fi
echo "Using remdia $revision for $target at $runtime"
