#!/usr/bin/env bash
# Cross-compile the Android JNI bridge (libreplayer_android.so), stage it plus
# the prebuilt remdia AAR and the rustls-platform-verifier JVM companion into
# the Gradle project, optionally assemble the APK.
# NDK environment mirrors remdia/scripts/build-android.py.
#
# usage: build-android.sh [--release] [--assemble] [--print-env]
#   --print-env  print the Android cross-compilation environment and exit, so
#                callers (scripts/android-dev.py) can run cargo/clippy with the
#                exact same settings instead of duplicating them.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

profile=debug
assemble=false
print_env=false
for arg in "$@"; do
  case $arg in
    --release) profile=release ;;
    --assemble) assemble=true ;;
    --print-env) print_env=true ;;
    *) echo "usage: build-android.sh [--release] [--assemble] [--print-env]" >&2; exit 2 ;;
  esac
done

sdk=${ANDROID_HOME:-$(cd "$root/../remdia/.deps/android-sdk" && pwd)}
ndk=${ANDROID_NDK_HOME:-$sdk/ndk/28.2.13676358}
java=${JAVA_HOME:-$(cd "$root/../remdia/.deps/android-jdk/Contents/Home" && pwd)}
case $(uname) in
  Darwin) host=darwin-x86_64 ;;
  Linux) host=linux-x86_64 ;;
  *) echo "unsupported host for NDK prebuilts: $(uname)" >&2; exit 1 ;;
esac
tools=$ndk/toolchains/llvm/prebuilt/$host/bin

# arm64-v8a only; 16 KB ELF alignment keeps Android 15+ page-size devices happy.
linker=$tools/aarch64-linux-android28-clang
rustflags="${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384"

if $print_env; then
  echo "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$linker"
  echo "CC_aarch64_linux_android=$linker"
  echo "AR_aarch64_linux_android=$tools/llvm-ar"
  echo "RUSTFLAGS=$rustflags"
  echo "ANDROID_SDK_ROOT=$sdk"
  echo "ANDROID_NDK_HOME=$ndk"
  echo "JAVA_HOME=$java"
  exit 0
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$linker
export CC_aarch64_linux_android=$linker
export AR_aarch64_linux_android=$tools/llvm-ar
export RUSTFLAGS=$rustflags

sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
sha1() { shasum -a 1 "$1" | awk '{print $1}'; }

cargo_args=(--locked -p replayer-android --target aarch64-linux-android)
if [[ $profile == release ]]; then cargo_args+=(--release); fi
cargo build "${cargo_args[@]}"

jni=android/app/src/main/jniLibs/arm64-v8a
mkdir -p "$jni"
cp "target/aarch64-linux-android/$profile/libreplayer_android.so" "$jni/"

# The AAR contributes dev.remdia Java classes and consumer proguard rules; its
# libremedia_sdk.so is excluded at packaging because the engine is statically
# embedded in libreplayer_android.so. Re-stage whenever the source differs: the
# previous "copy only if missing" rule silently kept stale engine builds.
aar=android/app/libs/remdia-0.3.0.aar
aar_source=$root/../remdia/dist/android/remdia-0.3.0.aar
mkdir -p "$(dirname "$aar")"
if [[ ! -f $aar_source ]]; then
  echo "remdia AAR not found at $aar_source" >&2
  echo "build it first: (cd ../remdia && ./scripts/build-android.py)" >&2
  exit 1
fi
if [[ ! -f $aar || $(sha256 "$aar") != "$(sha256 "$aar_source")" ]]; then
  cp "$aar_source" "$aar"
  echo "staged remdia AAR ($(sha256 "$aar"))"
fi

# The remdia Cargo pin and the local engine checkout should agree: the .so
# embeds the pinned git rev while the staged AAR was built from the checkout, so
# a drifted checkout can pair mismatched Java and native halves.
pin=$(sed -n 's/.*rev = "\([0-9a-f]*\)".*/\1/p' crates/replayer-android/Cargo.toml | head -1)
head=$(git -C "$root/../remdia" rev-parse HEAD 2>/dev/null || echo unknown)
if [[ -n $pin && $head != unknown && ${head:0:${#pin}} != "$pin" ]]; then
  echo "WARNING: remdia checkout $head differs from the pinned rev $pin" >&2
  echo "WARNING: the .so embeds the pinned rev, the AAR came from the checkout" >&2
fi

# rustls-platform-verifier needs its JVM companion (org.rustls.platformverifier)
# in the APK. Since companion 0.2.0 (upstream PR #253, which fixes CRL-only
# certificates being reported as Revoked, issue #221) the cargo package
# `rustls-platform-verifier-android` only synchronizes the version; the AAR is
# published on the upstream `maven-archive` branch. The version always comes
# from Cargo.lock, and downloads are verified against the published sha1.
stage_companion() {
  local version base rel dir aar expected
  version=$(cargo metadata --locked --format-version 1 --filter-platform aarch64-linux-android |
    python3 -c 'import json, sys
meta = json.load(sys.stdin)
for package in meta["packages"]:
    if package["name"] == "rustls-platform-verifier-android":
        print(package["version"])
        break
else:
    sys.exit("rustls-platform-verifier-android is absent from Cargo.lock")') || {
    echo "cannot resolve the rustls-platform-verifier companion version" >&2
    exit 1
  }
  base=https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven
  rel=org/rustls/rustls-platform-verifier/$version/rustls-platform-verifier-$version
  dir=android/app/libs/maven/org/rustls/rustls-platform-verifier/$version
  aar=$dir/rustls-platform-verifier-$version.aar

  # Reuse the staged copy only when it matches the published checksum, so an
  # interrupted or tampered download is never passed to Gradle.
  if [[ -f $aar && -f $aar.sha1 && $(sha1 "$aar") == "$(awk '{print $1}' "$aar.sha1")" ]]; then
    echo "TLS companion org.rustls:rustls-platform-verifier:$version already staged"
  else
    rm -rf android/app/libs/maven
    mkdir -p "$dir"
    local ext
    for ext in .aar .pom .aar.sha1; do
      curl -fsSL --retry 2 -o "$dir/rustls-platform-verifier-$version$ext" "$base/$rel$ext" || {
        echo "cannot download rustls-platform-verifier companion $version from $base" >&2
        rm -rf android/app/libs/maven
        exit 1
      }
    done
    expected=$(awk '{print $1}' "$aar.sha1")
    if [[ $(sha1 "$aar") != "$expected" ]]; then
      echo "companion AAR sha1 mismatch: want $expected got $(sha1 "$aar")" >&2
      rm -rf android/app/libs/maven
      exit 1
    fi
    echo "staged TLS companion org.rustls:rustls-platform-verifier:$version ($(sha256 "$aar"))"
  fi
  cat >android/app/libs/rustls-companion.properties <<EOF
# Generated by scripts/build-android.sh from Cargo.lock; do not edit.
# Gradle resolves the JVM companion of rustls-platform-verifier from
# android/app/libs/maven using this coordinate.
coordinate=org.rustls:rustls-platform-verifier:$version
sha256=$(sha256 "$aar")
source=$base/$rel.aar
EOF
}
stage_companion

if $assemble; then
  export JAVA_HOME=$java
  export ANDROID_HOME=$sdk
  export ANDROID_SDK_ROOT=$sdk
  task=assembleDebug
  if [[ $profile == release ]]; then task=assembleRelease; fi
  (cd android && ./gradlew --no-daemon "$task")

  # Publish the freshly built APK as the canonical artifact so run/audit never
  # pick up an older package after a failed build.
  apk=android/app/build/outputs/apk/$profile/app-$profile.apk
  out=target/android/$profile
  mkdir -p "$out"
  cp "$apk" "$out/replayer-$profile.apk"
  cat >"$out/artifact.json" <<EOF
{
  "profile": "$profile",
  "apk": "$out/replayer-$profile.apk",
  "sha256": "$(sha256 "$out/replayer-$profile.apk")",
  "bytes": $(stat -f%z "$out/replayer-$profile.apk" 2>/dev/null || stat -c%s "$out/replayer-$profile.apk"),
  "builtAt": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF
  echo "APK $out/replayer-$profile.apk ($(sha256 "$out/replayer-$profile.apk"))"
fi
echo "staged libreplayer_android.so ($profile) into $jni"
