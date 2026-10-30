"""Align, sign and verify an Android release APK, including older release tags."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def run(command, env=None):
    # Never echo signing credentials or tool error output containing them.
    result = subprocess.run([str(part) for part in command], env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if result.returncode:
        raise RuntimeError(f"{Path(command[0]).name} failed (exit {result.returncode}); output suppressed")
    return result.stdout


def signing_env():
    env = dict(os.environ)
    local = ROOT / ".deps/android-signing/keystore.properties"
    values = {}
    if local.exists():
        if os.name != "nt" and local.stat().st_mode & 0o077:
            raise ValueError("Local signing credentials must have mode 0600")
        values = dict(line.split("=", 1) for line in local.read_text().splitlines() if "=" in line)
    for name, field in [("ANDROID_KEYSTORE_FILE", "storeFile"),
                        ("ANDROID_KEYSTORE_PASSWORD", "storePassword"),
                        ("ANDROID_KEY_ALIAS", "keyAlias"), ("ANDROID_KEY_PASSWORD", "keyPassword")]:
        if not env.get(name):
            env[name] = values.get(field, "")
        if not env[name]:
            raise ValueError(f"Missing signing configuration: {name}")
    return env


def payload(path):
    with zipfile.ZipFile(path) as archive:
        result = {}
        for item in archive.infolist():
            if item.is_dir() or re.fullmatch(r"META-INF/(?:MANIFEST\.MF|[^/]+\.(?:SF|RSA|DSA|EC))", item.filename, re.I):
                continue
            if item.filename in result:
                raise ValueError("Duplicate APK entry")
            with archive.open(item) as stream:
                digest = hashlib.sha256()
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(chunk)
            result[item.filename] = digest.hexdigest()
        return result


def verify_metadata(badging, version):
    if not re.search(r"^package: name='dev\.replayer'.*versionName='" + re.escape(version) + r"'", badging, re.M):
        raise ValueError("APK package/version does not match the release tag")
    if "application-debuggable" in badging:
        raise ValueError("A debuggable APK cannot be published")
    if not re.search(r"^sdkVersion:'28'$", badging, re.M):
        raise ValueError("Unexpected Android minimum version")
    if not re.search(r"^native-code: 'arm64-v8a'\s*$", badging, re.M):
        raise ValueError("Unexpected Android ABI set")


def verify_certificate(output, expected, scheme):
    certificates = re.findall(r"^Signer #\d+ certificate SHA-256 digest: ([0-9a-fA-F]+)$", output, re.M)
    if [value.lower() for value in certificates] != [expected]:
        raise ValueError("APK signing certificate does not match the pinned release certificate")
    if f"Verified using v{scheme} scheme (APK Signature Scheme v{scheme}): true" not in output:
        raise ValueError(f"APK v{scheme} signature is required")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--sdk", type=Path, default=os.environ.get("ANDROID_HOME"))
    parser.add_argument("--output-directory", type=Path, default=ROOT / "dist/android")
    args = parser.parse_args()
    if not re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
        parser.error("Expected a stable vX.Y.Z release tag")
    if not args.sdk:
        parser.error("Set ANDROID_HOME or pass --sdk")
    tools = args.sdk / "build-tools/35.0.0"
    expected = (ROOT / "android/release-certificate.sha256").read_text().strip()
    if not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError("Invalid pinned signing certificate")
    env = signing_env()
    verify_metadata(run([tools / "aapt", "dump", "badging", args.apk], env), args.tag[1:])
    before = payload(args.apk)
    output = args.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=True)
    name = f"replayer-{args.tag[1:]}-android-arm64-v8a.apk"
    # Work under the checkout; only publish the final artifact after all checks pass.
    (ROOT / "target/android-release").mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=ROOT / "target/android-release") as work:
        aligned, signed = Path(work) / "aligned.apk", Path(work) / "signed.apk"
        run([tools / "zipalign", "-P", "16", "-f", "4", args.apk, aligned], env)
        run([tools / "apksigner", "sign", "--ks", env["ANDROID_KEYSTORE_FILE"],
             "--ks-key-alias", env["ANDROID_KEY_ALIAS"], "--ks-pass", "env:ANDROID_KEYSTORE_PASSWORD",
             "--key-pass", "env:ANDROID_KEY_PASSWORD", "--v2-signing-enabled", "true",
             "--v3-signing-enabled", "true", "--v4-signing-enabled", "false", "--out", signed, aligned], env)
        signature = run([tools / "apksigner", "verify", "--verbose", "--print-certs", signed], env)
        verify_certificate(signature, expected, 3)
        # On Android 9+ apksigner selects v3 and reports v2 as unverified.
        # Verify the v2 block independently rather than weakening either check.
        legacy = run([tools / "apksigner", "verify", "--verbose", "--print-certs",
                      "--min-sdk-version", "24", "--max-sdk-version", "27", signed], env)
        verify_certificate(legacy, expected, 2)
        run([tools / "zipalign", "-c", "-P", "16", "4", signed], env)
        verify_metadata(run([tools / "aapt", "dump", "badging", signed], env), args.tag[1:])
        if payload(signed) != before:
            raise ValueError("Signing unexpectedly changed APK application payloads")
        destination = output / name
        signed.replace(destination)
    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    (output / "SHA256SUMS-android-arm64-v8a.txt").write_text(f"{digest}  {name}\n")
    report = {"tag": args.tag, "apk": name, "bytes": destination.stat().st_size,
              "sha256": digest, "certificate_sha256": expected, "payload_unchanged": True}
    (output / "android-release.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
