#!/usr/bin/env bash
# Reuse Cargo's fixed executable; stage the matching plugins only after success.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
target=""
profile=debug
previous=""
for arg in "$@"; do
  if [[ $previous == --target ]]; then target=$arg; export REMDIA_TARGET=$arg; fi
  if [[ $arg == --target=* ]]; then target="${arg#--target=}"; export REMDIA_TARGET=$target; fi
  if [[ $arg == --release ]]; then profile=release; fi
  previous=$arg
done
./scripts/setup-macos.sh
case "${REMDIA_TARGET:-$(rustc -vV | awk '/^host:/ {print $2}')}" in
  aarch64-apple-darwin) arch=aarch64 ;;
  x86_64-apple-darwin) arch=x86_64 ;;
  *) exit 1 ;;
esac
source "$root/.deps/macos-$arch/env.sh"
cargo build --locked "$@"
python3 - "$profile" "$target" <<'STAGE'
import json, os, pathlib, shutil, subprocess, sys
metadata=json.loads(subprocess.check_output(['cargo','metadata','--no-deps','--format-version','1','--locked']))
build=pathlib.Path(metadata['target_directory'])
if sys.argv[2]: build/=sys.argv[2]
plugins=build/sys.argv[1]/'plugins'
plugins.mkdir(parents=True,exist_ok=True)
for path in pathlib.Path(os.environ['REMDIA_PLUGIN_DIR']).iterdir():
    if path.name in ('fallback_mp3_lame.json','libremdia_backend_fallback_mp3_lame.dylib'):
        destination=plugins/path.name
        destination.unlink(missing_ok=True)
        shutil.copy2(path,destination)
# Retire only SDK-owned libraries; preserve unrelated user extensions.
for name in ('native','macos','image','system_audio','system_demux','fallback_demux'):
    for filename in (name+'.json',f'libremdia_backend_{name}.dylib'):
        (plugins/filename).unlink(missing_ok=True)
for filename in ('sdk.json','libremdia_sdk.dylib'):
    (plugins/filename).unlink(missing_ok=True)
STAGE
