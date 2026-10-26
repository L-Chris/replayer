#!/usr/bin/env bash
# Reuse Cargo's fixed executable and the same dependencies locally and in CI.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
./scripts/setup-macos.sh
case $(uname -m) in
  arm64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) exit 1 ;;
esac
# shellcheck source=/dev/null
source "$root/.deps/macos-$arch/env.sh"
cargo build --locked "$@"
