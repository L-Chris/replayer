#!/usr/bin/env python3
"""Unified Android development entry point for pordo.

Every step from a locked Android cargo check to a verified device run goes
through this script so no second, inconsistent build path appears next to
``scripts/build-android.sh``. Subcommands report what they actually executed;
an unimplemented one fails loudly instead of exiting successfully.

Artifacts land under the checkout's ``target/``:

* ``target/android/<profile>/``   published APK plus ``artifact.json``
* ``target/android-audit/``       cargo/build logs and APK audit results
* ``target/android-review/``      device runs, instrumentation results, screenshots

Typical loop::

    scripts/android-dev.py doctor
    scripts/android-dev.py check
    scripts/android-dev.py build
    scripts/android-dev.py run --serial emulator-5554
    scripts/android-dev.py smoke --serial emulator-5554 --online
    scripts/android-dev.py audit
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
import zipfile
from contextlib import nullcontext
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = "dev.replayer"
ACTIVITY = f"{PACKAGE}/dev.pordo.MainActivity"
TEST_RUNNER = "dev.pordo.ProxyTestRunner"
TARGET_ABI = "arm64-v8a"
MIN_API = 28
PAGE_SIZE_16K = 16384
JNI_CLASSES = (
    "org/rustls/platformverifier/CertificateVerifier",
    "dev/remdia/Bridge",
    "dev/pordo/Native",
)


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def human(size: int) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if size < 1024 or unit == "GB":
            return f"{size:.1f} {unit}" if unit != "B" else f"{size} B"
        size /= 1024  # type: ignore[assignment]
    return f"{size} B"


class ToolError(RuntimeError):
    """A step failed; the message is already actionable."""


@dataclass
class Env:
    """Resolved toolchain locations plus the Android cross-compilation env."""

    sdk: Path
    ndk: Path
    jdk: Path
    adb: Path

    @property
    def build_env(self) -> dict[str, str]:
        return {
            "ANDROID_HOME": str(self.sdk),
            "ANDROID_SDK_ROOT": str(self.sdk),
            "ANDROID_NDK_HOME": str(self.ndk),
            "JAVA_HOME": str(self.jdk),
        }

    @property
    def build_tools(self) -> Path:
        candidates = sorted(
            (path for path in self.sdk.glob("build-tools/*") if path.is_dir()),
            key=lambda path: [int(part) for part in re.findall(r"\d+", path.name)[:3]],
        )
        if not candidates:
            raise ToolError(f"no build-tools under {self.sdk}; install build-tools;35.0.0")
        return candidates[-1]

    @property
    def emulator(self) -> Path:
        return self.sdk / "emulator" / "emulator"

    def tool(self, name: str) -> Path:
        path = self.build_tools / name
        if not path.exists():
            raise ToolError(f"missing {path}")
        return path

    def llvm(self, name: str) -> str:
        host = "darwin-x86_64" if sys.platform == "darwin" else "linux-x86_64"
        path = self.ndk / "toolchains" / "llvm" / "prebuilt" / host / "bin" / name
        if not path.exists():
            raise ToolError(f"missing NDK tool {path}")
        return str(path)


def run(
    command: list[str] | str,
    *,
    env: dict[str, str] | None = None,
    cwd: Path | None = None,
    log: Path | None = None,
    check: bool = True,
    quiet: bool = False,
    timeout: int | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run a command, optionally recording combined output in ``log``."""
    argv = shlex.split(command) if isinstance(command, str) else [str(part) for part in command]
    if not quiet:
        print(f"$ {' '.join(shlex.quote(part) for part in argv)}", flush=True)
    merged = dict(os.environ)
    if env:
        merged.update(env)
    if log is not None:
        log.parent.mkdir(parents=True, exist_ok=True)
    try:
        # Let the child write logs directly: they remain readable during long
        # builds, without a blocking readline loop defeating the deadline.
        with (log.open("w", encoding="utf-8") if log else nullcontext(subprocess.PIPE)) as output:
            result = subprocess.run(
                argv,
                cwd=str(cwd or ROOT),
                env=merged,
                stdout=output,
                stderr=subprocess.STDOUT,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=timeout,
            )
    except subprocess.TimeoutExpired as error:
        hint = f" (partial log: {log})" if log else ""
        raise ToolError(f"command timed out after {timeout}s{hint}: {shlex.join(argv)}") from error
    if log:
        result.stdout = log.read_text(encoding="utf-8", errors="replace")
    if not quiet:
        print(result.stdout, end="", flush=True)
    code = result.returncode
    if check and code != 0:
        hint = f" (log: {log})" if log else ""
        raise ToolError(f"command failed with exit {code}{hint}: {' '.join(argv)}")
    return result


def resolve_env(args: argparse.Namespace) -> Env:
    sdk = Path(args.sdk).resolve()
    if not sdk.is_dir():
        raise ToolError(f"Android SDK not found at {sdk}; pass --sdk")
    ndk = Path(args.ndk).resolve() if args.ndk else _default_ndk(sdk)
    jdk = Path(args.jdk).resolve() if args.jdk else _default_jdk()
    adb = Path(args.adb).resolve() if args.adb else _default_adb(sdk)
    if not ndk.is_dir():
        raise ToolError(f"NDK not found at {ndk}; pass --ndk")
    if not (jdk / "bin" / "java").exists():
        raise ToolError(f"JDK not found at {jdk}; pass --jdk")
    if not adb.exists():
        raise ToolError(f"adb not found at {adb}; pass --adb")
    return Env(sdk=sdk, ndk=ndk, jdk=jdk, adb=adb)


def _default_ndk(sdk: Path) -> Path:
    pinned = sdk / "ndk" / "28.2.13676358"
    if pinned.is_dir():
        return pinned
    found = sorted(sdk.glob("ndk/*"))
    if not found:
        raise ToolError(f"no NDK under {sdk}/ndk; pass --ndk")
    return found[-1]


def _default_jdk() -> Path:
    bundled = ROOT.parent / "remdia" / ".deps" / "android-jdk" / "Contents" / "Home"
    if (bundled / "bin" / "java").exists():
        return bundled
    java_home = os.environ.get("JAVA_HOME")
    if java_home and (Path(java_home) / "bin" / "java").exists():
        return Path(java_home)
    raise ToolError("no JDK found; pass --jdk or set JAVA_HOME")


def _default_adb(sdk: Path) -> Path:
    """Pick a working adb.

    The SDK-bundled adb 37.0.1 has aborted with a libunwind error on this host,
    while the separately fetched 36.0.2 platform-tools is reliable. Prefer the
    known-good binary when it is present so `doctor`/`run`/`smoke` do not depend
    on the caller remembering `--adb`; `--adb` still overrides everything.
    """
    stable = ROOT.parent / "remdia" / ".deps" / "android-platform-tools-36" / "platform-tools" / "adb"
    if stable.exists():
        return stable
    return sdk / "platform-tools" / "adb"


def cross_env(env: Env) -> dict[str, str]:
    """Ask build-android.sh for its environment so cargo settings stay single-sourced."""
    script = ROOT / "scripts" / "build-android.sh"
    if not script.exists():
        raise ToolError(f"missing {script}")
    result = run([str(script), "--print-env"], env=env.build_env, quiet=True)
    if result.returncode != 0:
        raise ToolError(f"{script.name} --print-env failed:\n{result.stdout}")
    variables: dict[str, str] = {}
    for line in result.stdout.splitlines():
        if "=" in line:
            key, _, value = line.partition("=")
            variables[key.strip()] = value
    for required in ("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", "RUSTFLAGS"):
        if required not in variables:
            raise ToolError(f"{script.name} --print-env did not report {required}")
    variables["JAVA_HOME"] = str(env.jdk)
    variables["ANDROID_HOME"] = str(env.sdk)
    variables["ANDROID_SDK_ROOT"] = str(env.sdk)
    return variables


# --------------------------------------------------------------------------- #
# devices


def adb(env: Env, args: list[str], serial: str | None = None, **kwargs) -> subprocess.CompletedProcess[str]:
    command = [str(env.adb)]
    if serial:
        command += ["-s", serial]
    command += args
    return run(command, quiet=True, check=False, **kwargs)


def devices(env: Env) -> list[dict[str, str]]:
    result = adb(env, ["devices", "-l"])
    if result.returncode != 0:
        raise ToolError(f"adb devices failed: {result.stdout.strip()}")
    found: list[dict[str, str]] = []
    for line in result.stdout.splitlines()[1:]:
        parts = line.split()
        if len(parts) >= 2 and parts[1] == "device":
            found.append({"serial": parts[0], "description": " ".join(parts[2:])})
    return found


def device_info(env: Env, serial: str) -> dict[str, str]:
    props = {
        "api": "ro.build.version.sdk",
        "release": "ro.build.version.release",
        "abi": "ro.product.cpu.abi",
        "model": "ro.product.model",
    }
    info: dict[str, str] = {"serial": serial}
    for key, prop in props.items():
        info[key] = adb(env, ["shell", "getprop", prop], serial).stdout.strip()
    page_size = adb(env, ["shell", "getconf", "PAGE_SIZE"], serial).stdout.strip()
    if not page_size.isdigit():
        # API 28 images ship without getconf; the kernel page size of any
        # mapping in smaps is the same value (4096 on all 4 KB images).
        smaps = adb(env, ["shell", "cat", "/proc/self/smaps"], serial).stdout
        match = re.search(r"KernelPageSize:\s+(\d+) kB", smaps)
        page_size = str(int(match.group(1)) * 1024) if match else "unknown"
    info["page_size"] = page_size
    return info


def require_device(env: Env, serial: str | None) -> str:
    attached = devices(env)
    if not attached:
        raise ToolError(
            "no device attached; start an emulator first, for example\n"
            f"  ANDROID_AVD_HOME={ROOT.parent}/remdia/target/android/avd "
            f"ANDROID_SDK_ROOT={env.sdk} {env.emulator} -avd remdia-api35 -no-window -no-audio -no-boot-anim -no-metrics"
        )
    if serial:
        if serial not in {item["serial"] for item in attached}:
            raise ToolError(f"{serial} is not attached; found {', '.join(item['serial'] for item in attached)}")
        return serial
    if len(attached) > 1:
        raise ToolError("multiple devices attached; pass --serial: " + ", ".join(item["serial"] for item in attached))
    return attached[0]["serial"]


def check_device_capabilities(info: dict[str, str]) -> list[str]:
    problems: list[str] = []
    if info.get("abi") != TARGET_ABI:
        problems.append(f"device ABI is {info.get('abi')}, the APK ships {TARGET_ABI} only")
    api = int(info.get("api") or 0)
    if api < MIN_API:
        problems.append(f"API {api} is below minSdk {MIN_API}")
    return problems


# --------------------------------------------------------------------------- #
# artifacts


def artifact(env: Env, profile: str) -> dict:
    path = ROOT / "target" / "android" / profile / "artifact.json"
    if not path.exists():
        raise ToolError(f"no published {profile} APK ({path}); run: scripts/android-dev.py build --profile {profile}")
    data = json.loads(path.read_text(encoding="utf-8"))
    apk = ROOT / data["apk"] if not Path(data["apk"]).is_absolute() else Path(data["apk"])
    if not apk.exists():
        raise ToolError(f"artifact.json points at a missing APK: {apk}")
    if apk.stat().st_size != int(data["bytes"]) or sha256(apk) != data["sha256"]:
        raise ToolError(f"published APK does not match artifact.json: {apk}; rebuild before installing")
    data["path"] = apk
    return data


def newest_source_mtime() -> float:
    """Newest mtime among inputs that change the *app* APK.

    Deliberately excludes `androidTest`/`test` sources: those only affect the
    instrumentation APK (which `smoke` rebuilds every run), so editing a test
    must not mark the already-correct app APK stale.
    """
    roots = [
        ROOT / "Cargo.toml",
        ROOT / "Cargo.lock",
        ROOT / "scripts" / "build-android.sh",
        ROOT / "android" / "app" / "libs",
        ROOT / "crates",
        ROOT / "android" / "app" / "src" / "main",
        ROOT / "android" / "app" / "build.gradle.kts",
        ROOT / "android" / "app" / "proguard-rules.pro",
        ROOT / "android" / "build.gradle.kts",
        ROOT / "android" / "settings.gradle.kts",
    ]
    newest = 0.0
    for root in roots:
        if root.is_file():
            newest = max(newest, root.stat().st_mtime)
            continue
        for path in root.rglob("*"):
            # jniLibs holds build-android.sh's staged output, not source; a
            # release stage must not mark the debug APK stale (and vice versa).
            if (path.is_file() and "build" not in path.parts and "jniLibs" not in path.parts
                    and path.name != "rustls-companion.properties"):
                newest = max(newest, path.stat().st_mtime)
    return newest


def reject_stale_apk(data: dict, allow: bool) -> None:
    built = datetime.strptime(data["builtAt"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc).timestamp()
    source = newest_source_mtime()
    if source > built:
        message = (
            "the published APK is older than the sources; a failed build must never be "
            "installed as if it were current"
        )
        if not allow:
            raise ToolError(f"{message}\n  rebuild with: scripts/android-dev.py build")
        print(f"WARNING: {message} (--allow-stale given)", file=sys.stderr)


# --------------------------------------------------------------------------- #
# subcommands


def command_doctor(args: argparse.Namespace, env: Env) -> int:
    problems: list[str] = []
    print("== toolchain")
    print(f"  python     {sys.version.split()[0]} ({sys.platform})")
    for label, path in (("sdk", env.sdk), ("ndk", env.ndk), ("jdk", env.jdk), ("adb", env.adb)):
        print(f"  {label:<10} {path}{' (missing)' if not path.exists() else ''}")
        if not path.exists():
            problems.append(f"{label} missing at {path}")
    ndk_props = env.ndk / "source.properties"
    if ndk_props.exists():
        print(f"  ndk rev    {ndk_props.read_text().splitlines()[-1].strip()}")
    java = run([str(env.jdk / "bin" / "java"), "-version"], quiet=True, check=False)
    print(f"  java       {(java.stdout or '').splitlines()[0] if java.stdout else 'unknown'}")
    print(f"  adb        {(adb(env, ['version']).stdout.splitlines() or ['unknown'])[1].strip()}")
    print(f"  build-tools {env.build_tools.name}")

    print("== rust")
    rustc = run(["rustc", "-vV"], quiet=True, check=False)
    if rustc.returncode != 0:
        problems.append("rustc not on PATH")
    else:
        print(f"  {rustc.stdout.splitlines()[0]}")
    targets = run(["rustup", "target", "list", "--installed"], quiet=True, check=False)
    installed = targets.stdout.split()
    if "aarch64-linux-android" in installed:
        print("  target     aarch64-linux-android installed")
    else:
        problems.append("rustup target add aarch64-linux-android")
        print("  target     aarch64-linux-android MISSING")

    print("== gradle")
    wrapper = ROOT / "android" / "gradle" / "wrapper" / "gradle-wrapper.properties"
    if wrapper.exists():
        for line in wrapper.read_text().splitlines():
            if line.startswith("distributionUrl"):
                print(f"  {line.split('/')[-1]}")
    else:
        problems.append(f"missing {wrapper}")

    print("== staged dependencies")
    companion = ROOT / "android" / "app" / "libs" / "rustls-companion.properties"
    if companion.exists():
        props = dict(
            line.split("=", 1) for line in companion.read_text().splitlines() if "=" in line and not line.startswith("#")
        )
        staged = sorted((ROOT / "android" / "app" / "libs" / "maven").rglob("*.aar"))
        digest = sha256(staged[0]) if staged else "missing"
        print(f"  companion  {props.get('coordinate')}")
        print(f"  companion  sha256 {digest}")
        if staged and digest != props.get("sha256"):
            problems.append("staged TLS companion AAR differs from the recorded sha256")
    else:
        problems.append("run scripts/build-android.sh once to stage the TLS companion")
        print("  companion  NOT STAGED")
    remdia = ROOT / "android" / "app" / "libs" / "remdia-0.3.0.aar"
    if remdia.exists():
        print(f"  remdia AAR sha256 {sha256(remdia)}")
    else:
        problems.append("remdia AAR not staged; run scripts/build-android.sh")
    native = ROOT / "android" / "app" / "src" / "main" / "jniLibs" / TARGET_ABI / "libpordo_android.so"
    if native.exists():
        built = ROOT / "target" / "aarch64-linux-android" / args.profile / "libpordo_android.so"
        stale = built.exists() and built.stat().st_mtime > native.stat().st_mtime
        print(f"  native     {human(native.stat().st_size)} sha256 {sha256(native)[:16]}{' STALE' if stale else ''}")
        if stale:
            problems.append("staged libpordo_android.so is older than the last cargo build")
    else:
        print("  native     not staged yet")

    print("== devices")
    attached = devices(env)
    if not attached:
        print("  none attached (emulator not started)")
    for item in attached:
        info = device_info(env, item["serial"])
        print(
            f"  {info['serial']}  API {info['api']} (Android {info['release']})  {info['abi']}"
            f"  page {info['page_size']}  {info['model']}"
        )
        for problem in check_device_capabilities(info):
            problems.append(f"{info['serial']}: {problem}")

    print("== AVDs")
    for home in (ROOT.parent / "remdia" / "target" / "android" / "avd", Path.home() / ".android" / "avd"):
        if home.is_dir():
            names = sorted(path.name for path in home.glob("*.ini"))
            print(f"  {home}: {', '.join(names) if names else 'none'}")

    if problems:
        print("\ndoctor found problems:")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print("\ndoctor: all required dependencies resolved")
    return 0


def command_check(args: argparse.Namespace, env: Env) -> int:
    variables = cross_env(env)
    stamp = now()
    log = ROOT / "target" / "android-audit" / f"check-{args.profile}-{stamp}.log"
    tool = "clippy" if args.clippy else "check"
    command = [
        "cargo",
        tool,
        "--locked",
        "-p",
        "pordo-android",
        "--target",
        "aarch64-linux-android",
    ]
    if args.profile == "release":
        command.append("--release")
    if args.clippy:
        command += ["--", "-W", "clippy::all"]
    print(f"android {tool} (log: {log})")
    result = run(command, env=variables, log=log, check=False)
    shutil.copyfile(log, ROOT / "target" / "android-audit" / "current-check.log")
    if result.returncode != 0:
        print(f"android {tool} FAILED; nothing was staged or installed", file=sys.stderr)
        return result.returncode
    warnings = len(re.findall(r"^warning", result.stdout, re.MULTILINE))
    print(f"android {tool} passed ({warnings} warning(s)); this is a host-side cargo check for the Android target")
    return 0


def command_build(args: argparse.Namespace, env: Env) -> int:
    started = time.time()
    command = [str(ROOT / "scripts" / "build-android.sh"), "--assemble"]
    if args.profile == "release":
        command.append("--release")
    stamp = now()
    log = ROOT / "target" / "android-audit" / f"build-{args.profile}-{stamp}.log"
    print(f"building {args.profile} (log: {log})")
    result = run(command, env=env.build_env, log=log, check=False)
    elapsed = time.time() - started
    if result.returncode != 0:
        print("build FAILED; no APK was published and no device install may proceed", file=sys.stderr)
        return result.returncode
    data = artifact(env, args.profile)
    published = Path(data["path"])
    if published.stat().st_mtime < started - 1:
        raise ToolError(f"build succeeded but {published} was not refreshed; refusing to report success")
    data["build_seconds"] = round(elapsed, 1)
    print(
        f"\nAPK      {published}\n"
        f"sha256   {data['sha256']}\n"
        f"size     {human(int(data['bytes']))}\n"
        f"elapsed  {data['build_seconds']}s (cargo + gradle, warm caches)"
    )
    return 0


def command_run(args: argparse.Namespace, env: Env) -> int:
    data = artifact(env, args.profile)
    reject_stale_apk(data, args.allow_stale)
    serial = require_device(env, args.serial)
    info = device_info(env, serial)
    print(f"device   {json.dumps(info, ensure_ascii=False)}")
    for problem in check_device_capabilities(info):
        raise ToolError(problem)
    apk = Path(data["path"])
    print(f"installing {apk.name} ({human(apk.stat().st_size)}) on {serial}")
    installed = adb(env, ["install", "--no-incremental", "-r", str(apk)], serial, timeout=900)
    if installed.returncode != 0 or "Success" not in installed.stdout:
        raise ToolError(f"install failed: {installed.stdout.strip()}")

    grant_permissions(env, serial, info)
    adb(env, ["shell", "am", "force-stop", PACKAGE], serial)
    adb(env, ["logcat", "-c"], serial)
    started = adb(env, ["shell", "am", "start", "-n", ACTIVITY], serial)
    if started.returncode != 0:
        raise ToolError(f"am start failed: {started.stdout.strip()}")
    pid = wait_for_process(env, serial, args.timeout)
    resumed = wait_for_activity(env, serial, args.timeout)
    gate = read_init_gate(env, serial)
    crash = collect_crash_lines(env, serial)

    report = {
        "startedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "device": info,
        "apk": str(apk),
        "sha256": data["sha256"],
        "bytes": int(data["bytes"]),
        "pid": pid,
        "resumedActivity": resumed,
        "initGate": gate,
        "crashLines": crash,
    }
    out = ROOT / "target" / "android-review" / f"run-{args.profile}-{now()}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    shot = capture_screenshot(env, serial, out.with_suffix(".png"))

    print(f"\npid      {pid}\nactivity {resumed}\nreport   {out}")
    if shot:
        print(f"screen   {shot}")
    if gate:
        print(f"INIT GATE VISIBLE: {gate}", file=sys.stderr)
        return 1
    if not pid:
        print("process is not running after launch", file=sys.stderr)
        return 1
    if resumed != ACTIVITY:
        print(f"expected {ACTIVITY} to be resumed, saw {resumed}", file=sys.stderr)
        return 1
    if crash:
        print("crash buffer is not empty:", file=sys.stderr)
        for line in crash[:20]:
            print(f"  {line}", file=sys.stderr)
        return 1
    print("run: launched, native bridge ready, no crash output")
    return 0


def grant_permissions(env: Env, serial: str, info: dict[str, str]) -> None:
    wanted = ["android.permission.POST_NOTIFICATIONS"] if int(info.get("api") or 0) >= 33 else []
    wanted.append(
        "android.permission.READ_MEDIA_AUDIO"
        if int(info.get("api") or 0) >= 33
        else "android.permission.READ_EXTERNAL_STORAGE"
    )
    for permission in wanted:
        result = adb(env, ["shell", "pm", "grant", PACKAGE, permission], serial)
        status = "granted" if result.returncode == 0 else "skipped"
        print(f"  {status:<8} {permission}")


def wait_for_process(env: Env, serial: str, timeout: int) -> str:
    deadline = time.time() + timeout
    while time.time() < deadline:
        pid = adb(env, ["shell", "pidof", PACKAGE], serial).stdout.strip()
        if pid:
            return pid
        time.sleep(1)
    return ""


def wait_for_activity(env: Env, serial: str, timeout: int) -> str:
    deadline = time.time() + timeout
    resumed = ""
    while time.time() < deadline:
        dump = adb(env, ["shell", "dumpsys", "activity", "activities"], serial).stdout
        match = re.search(r"(?:topResumedActivity|mResumedActivity)[:=]\s*ActivityRecord\{[^}]*?\s([\w.]+/[\w.$]+)", dump)
        if match:
            resumed = match.group(1)
            if resumed == ACTIVITY or PACKAGE in resumed:
                return resumed
        time.sleep(1)
    return resumed


def read_init_gate(env: Env, serial: str) -> str:
    """Return the initialization error text when the startup gate is showing."""
    adb(env, ["shell", "uiautomator", "dump", "/sdcard/pordo-ui.xml"], serial)
    dump = adb(env, ["exec-out", "cat", "/sdcard/pordo-ui.xml"], serial).stdout
    if "init-error-title" not in dump:
        return ""
    try:
        root = ET.fromstring(dump)
    except ET.ParseError:
        return "initialization gate visible (detail unavailable)"
    # `testTagsAsResourceId` publishes the raw tag as the resource-id (Compose
    # calls setViewIdResourceName with the tag string itself, no `package:id/`
    # prefix), so match the bare tag and tolerate a prefixed form.
    for node in root.iter("node"):
        resource = node.get("resource-id") or ""
        if resource == "init-error-detail" or resource.endswith(":id/init-error-detail"):
            return node.get("text") or "initialization gate visible"
    return "initialization gate visible"


def collect_crash_lines(env: Env, serial: str) -> list[str]:
    crash = adb(env, ["logcat", "-d", "-b", "crash"], serial).stdout
    native = adb(env, ["logcat", "-d", "-s", "pordo-native:E", "AndroidRuntime:E", "DEBUG:F"], serial).stdout
    # "--------- beginning of ..." banners are logcat metadata, not events; on
    # freshly cleared buffers they can be the only output (seen on API 28).
    lines = [
        line
        for line in (crash + native).splitlines()
        if line.strip() and not line.startswith("--------- beginning of")
    ]
    return lines


def capture_screenshot(env: Env, serial: str, destination: Path):
    remote = "/sdcard/pordo-screen.png"
    if adb(env, ["shell", "screencap", "-p", remote], serial).returncode != 0:
        return None
    destination.parent.mkdir(parents=True, exist_ok=True)
    pulled = adb(env, ["pull", remote, str(destination)], serial)
    return destination if pulled.returncode == 0 and destination.exists() else None


def command_logs(args: argparse.Namespace, env: Env) -> int:
    serial = require_device(env, args.serial)
    out = ROOT / "target" / "android-review" / f"logs-{now()}.log"
    out.parent.mkdir(parents=True, exist_ok=True)
    pid = adb(env, ["shell", "pidof", PACKAGE], serial).stdout.strip()
    sections = {
        "crash buffer": ["logcat", "-d", "-b", "crash"],
        "native + runtime": ["logcat", "-d", "-s", "pordo-native:V", "AndroidRuntime:E", "DEBUG:V", "pordo:V"],
    }
    if pid:
        sections[f"app pid {pid}"] = ["logcat", "-d", "--pid", pid]
    with out.open("w", encoding="utf-8") as sink:
        for title, command in sections.items():
            body = adb(env, command, serial).stdout
            sink.write(f"===== {title} =====\n{body}\n")
    text = out.read_text(encoding="utf-8")
    interesting = [
        line
        for line in text.splitlines()
        if re.search(r"panic|FATAL|Exception|abort message|signal \d|pordo-native", line)
    ]
    print(f"logs     {out}")
    print(f"device   {serial} pid {pid or 'not running'}")
    if interesting:
        print("\nroot causes:")
        for line in interesting[:40]:
            print(f"  {line}")
    else:
        print("\nno panic, exception or fatal line captured")
    symbols = ROOT / "target" / "aarch64-linux-android" / args.profile / "libpordo_android.so"
    print("\nsymbolize a native frame with:")
    print(f"  {env.llvm('llvm-addr2line')} -Cfe {symbols} <address>")
    print(f"  {env.sdk}/ndk/{env.ndk.name}/ndk-stack -sym {symbols.parent} -dump <logcat.txt>")
    return 0


def command_smoke(args: argparse.Namespace, env: Env) -> int:
    if args.profile != "debug":
        raise ToolError("smoke supports the debug instrumentation variant only; use run/audit for release")
    serial = require_device(env, args.serial)
    info = device_info(env, serial)
    for problem in check_device_capabilities(info):
        raise ToolError(problem)
    data = artifact(env, args.profile)
    reject_stale_apk(data, args.allow_stale)

    stamp = now()
    gradle_env = env.build_env
    build_log = ROOT / "target" / "android-audit" / f"androidtest-{stamp}.log"
    print("assembling the instrumentation APK")
    run(
        ["./gradlew", "--no-daemon", f"assemble{args.profile.capitalize()}AndroidTest"],
        cwd=ROOT / "android",
        env=gradle_env,
        log=build_log,
    )
    test_apk = ROOT / "android" / "app" / "build" / "outputs" / "apk" / "androidTest" / args.profile / f"app-{args.profile}-androidTest.apk"
    if not test_apk.exists():
        raise ToolError(f"expected instrumentation APK at {test_apk}")

    print(f"installing app + test APKs on {serial}")
    for apk in (Path(data["path"]), test_apk):
        installed = adb(env, ["install", "--no-incremental", "-r", str(apk)], serial, timeout=900)
        if installed.returncode != 0 or "Success" not in installed.stdout:
            raise ToolError(f"install failed for {apk.name}: {installed.stdout.strip()}")
    grant_permissions(env, serial, info)

    arguments = ["-e", "class", args.test] if args.test else []
    if args.online:
        arguments += ["-e", "online", "true"]
    if args.proxy:
        # Route the core's raw reqwest sockets through a host proxy; see
        # ProxyTestRunner. Without it the online case can only reach sites the
        # emulator can resolve and route to directly.
        arguments += ["-e", "proxy", args.proxy]
    adb(env, ["shell", "am", "force-stop", PACKAGE], serial)
    adb(env, ["shell", "am", "force-stop", f"{PACKAGE}.test"], serial)
    adb(env, ["logcat", "-c"], serial)
    before = capture_screenshot(env, serial, ROOT / "target" / "android-review" / f"smoke-before-{stamp}.png")
    mode = "online" if args.online else "offline"
    if args.online and args.proxy:
        mode += f", proxy {args.proxy}"
    print(f"running instrumentation ({mode})")
    raw = ROOT / "target" / "android-review" / f"smoke-{stamp}.log"
    try:
        result = adb(
            env,
            ["shell", "am", "instrument", "-w", "-r", *arguments, f"{PACKAGE}.test/{TEST_RUNNER}"],
            serial,
            log=raw,
            timeout=args.timeout,
        )
    except ToolError:
        # Killing the adb client alone leaves instrumentation alive on-device.
        adb(env, ["shell", "am", "force-stop", PACKAGE], serial, timeout=10)
        raise
    after = capture_screenshot(env, serial, ROOT / "target" / "android-review" / f"smoke-after-{stamp}.png")

    summary = parse_instrumentation(result.stdout)
    summary["device"] = info
    summary["app"] = {"sha256": data["sha256"], "apk": str(data["path"])}
    summary["online"] = bool(args.online)
    summary["proxy"] = args.proxy
    summary["raw"] = str(raw)
    summary["screenshots"] = [str(path) for path in (before, after) if path]
    report = ROOT / "target" / "android-review" / f"smoke-{stamp}.json"
    report.write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")

    print(f"\n{'case':<64} result")
    for case in summary["cases"]:
        label = f"{case['class'].rsplit('.', 1)[-1]}.{case['test']}"
        print(f"{label:<64} {case['status']}")
    print(
        f"\ntotal {summary['total']}  passed {summary['passed']}  failed {summary['failed']}  "
        f"assumption-skipped {summary['skipped']}  ignored {summary['ignored']}"
    )
    if summary["expected"] and summary["expected"] != summary["total"]:
        print(
            f"WARNING: the runner planned {summary['expected']} cases but only "
            f"{summary['total']} reached a verdict",
            file=sys.stderr,
        )
    print(f"report   {report}\nraw      {raw}")
    for path in summary["screenshots"]:
        print(f"screen   {path}")
    for failure in summary["failures"]:
        print(f"\nFAILURE {failure['case']}\n{failure['trace']}", file=sys.stderr)
    for line in summary["terminal"]:
        print(f"\nRUNNER TERMINATED: {line}", file=sys.stderr)
    if summary["incomplete"]:
        print(f"\nNO VERDICT for: {', '.join(summary['incomplete'])}", file=sys.stderr)
    crash = collect_crash_lines(env, serial)
    if crash:
        print("\ncrash/native error output during the run:", file=sys.stderr)
        for line in crash[:20]:
            print(f"  {line}", file=sys.stderr)
    if result.returncode != 0 or summary["failed"] or crash or not summary["complete"]:
        if summary["total"] == 0:
            print("no instrumentation case reported a verdict", file=sys.stderr)
        return 1
    return 0


STATUS_LABELS = {
    1: "started",
    2: "in-progress",
    0: "passed",
    -1: "failed",
    -2: "failed",
    -3: "ignored",
    -4: "skipped",
}


def parse_instrumentation(output: str) -> dict:
    """Turn `am instrument -w -r` output into per-case results.

    The runner emits `INSTRUMENTATION_STATUS: key=value` blocks whose values can
    span lines (`stream`, `stack`), each terminated by
    `INSTRUMENTATION_STATUS_CODE`. Codes: 1/2 start, 0 pass, -1/-2 fail,
    -3 ignored, -4 assumption failure (counted as skipped, matching JUnit).
    A run that never emits `INSTRUMENTATION_CODE` crashed or was killed, which
    must not be reported as a pass.
    """
    cases: list[dict] = []
    by_key: dict[tuple[str, str], dict] = {}
    failures: list[dict] = []
    terminal: list[str] = []
    summary: list[str] = []
    status: dict[str, str] = {}
    result: dict[str, str] = {}
    target = status
    key: str | None = None
    lines: list[str] = []
    finished = False
    expected = 0
    block_open = False

    def close() -> None:
        nonlocal key
        if key is not None:
            target[key] = "\n".join(lines).strip("\n")
        key, lines[:] = None, []

    for line in output.splitlines():
        if line.startswith("INSTRUMENTATION_STATUS: "):
            if not block_open:
                # A fresh status block describes one transition of one case;
                # drop the previous block so a stale `stack` cannot be
                # attributed to a case that passed.
                status.clear()
                block_open = True
            close()
            target = status
            key, _, value = line[len("INSTRUMENTATION_STATUS: ") :].partition("=")
            lines.append(value)
        elif line.startswith("INSTRUMENTATION_RESULT: "):
            close()
            target = result
            key, _, value = line[len("INSTRUMENTATION_RESULT: ") :].partition("=")
            lines.append(value)
        elif line.startswith("INSTRUMENTATION_STATUS_CODE: "):
            close()
            block_open = False
            expected = int(status.get("numtests") or expected)
            code = int(line.rsplit(" ", 1)[-1])
            name = (status.get("class", "?"), status.get("test", "?"))
            case = by_key.get(name)
            if code in (1, 2):
                if case is None:
                    case = {
                        "class": name[0],
                        "test": name[1],
                        "status": STATUS_LABELS[code],
                        "seconds": None,
                    }
                    by_key[name] = case
                    cases.append(case)
                continue
            if case is None:
                case = {
                    "class": name[0],
                    "test": name[1],
                    "status": STATUS_LABELS.get(code, str(code)),
                    "seconds": None,
                }
                by_key[name] = case
                cases.append(case)
            case["status"] = STATUS_LABELS.get(code, str(code))
            # Buffered instrumentation text has no per-case timestamps; parsing
            # speed is not test duration. Keep seconds unknown rather than 0.00.
            if case["status"] == "failed":
                failures.append(
                    {
                        "case": f"{name[0]}.{name[1]}",
                        "trace": (status.get("stack") or status.get("stream") or "").strip(),
                    }
                )
        elif line.startswith("INSTRUMENTATION_CODE: "):
            close()
            finished = line.partition(": ")[2].strip() == "-1"
        elif line.startswith(("INSTRUMENTATION_FAILED", "INSTRUMENTATION_ABORTED", "INSTRUMENTATION_CRASHED")):
            close()
            terminal.append(line.strip())
        else:
            if key is not None:
                lines.append(line)
            elif line.strip():
                summary.append(line.strip())
    close()

    counted = {"passed": 0, "failed": 0, "ignored": 0, "skipped": 0}
    for case in cases:
        counted[case["status"]] = counted.get(case["status"], 0) + 1
    settled = sum(counted.values())
    incomplete = [
        f"{case['class']}.{case['test']}"
        for case in cases
        if case["status"] in ("started", "in-progress")
    ]
    return {
        "total": settled,
        "passed": counted["passed"],
        "failed": counted["failed"],
        "skipped": counted["skipped"],
        "ignored": counted["ignored"],
        "expected": expected or None,
        "complete": finished and not terminal and not incomplete and settled > 0
        and (not expected or expected == settled),
        "terminal": terminal,
        "incomplete": incomplete,
        "cases": cases,
        "failures": failures,
        "runnerResult": {k: v for k, v in result.items() if k in ("stream", "shortMsg", "longMsg")},
        "summary": summary[-6:],
        "tail": "\n".join(output.splitlines()[-15:]),
    }


def apk_size_buckets(entries: list[zipfile.ZipInfo]) -> dict[str, dict[str, int]]:
    buckets: dict[str, dict[str, int]] = {}
    for entry in entries:
        key = (
            "native" if entry.filename.endswith(".so")
            else "dex" if entry.filename.endswith(".dex")
            else "resources" if entry.filename.startswith("res/")
            or entry.filename in {"resources.arsc", "AndroidManifest.xml"}
            else "other"
        )
        bucket = buckets.setdefault(key, {"files": 0, "compressed": 0, "uncompressed": 0})
        bucket["files"] += 1
        bucket["compressed"] += entry.compress_size
        bucket["uncompressed"] += entry.file_size
    return dict(sorted(buckets.items()))


def dex_classes(env: Env, apk: Path) -> set[str]:
    output = run([str(env.tool("dexdump")), str(apk)], quiet=True, timeout=900).stdout
    return set(re.findall(r"Class descriptor\s*:\s*'L([^']+);'", output))


def command_audit(args: argparse.Namespace, env: Env) -> int:
    apk = Path(args.apk) if args.apk else Path(artifact(env, args.profile)["path"])
    if not apk.exists():
        raise ToolError(f"APK not found: {apk}")
    print(f"auditing {apk} ({human(apk.stat().st_size)})")
    source_root = Path(args.source_root)
    native_paths = list((source_root / "android/app/src/main/java").glob("**/Native.kt"))
    if len(native_paths) != 1:
        raise ToolError("Expected one source Native.kt for this APK")
    kotlin = native_paths[0].read_text(encoding="utf-8")
    namespace = re.search(r"^package ([\w.]+)", kotlin, re.M).group(1)
    library = "lib" + re.search(r'System.loadLibrary\("([\w]+)"\)', kotlin).group(1) + ".so"
    native_class = namespace.replace(".", "/") + "/Native"
    checks: list[dict] = []

    def record(name: str, ok: bool, detail: str) -> None:
        checks.append({"name": name, "ok": ok, "detail": detail})
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}: {detail}")

    with zipfile.ZipFile(apk) as archive:
        entries = archive.infolist()
        names = [entry.filename for entry in entries]
        dex_names = [name for name in names if name.endswith(".dex")]
        so_names = [name for name in names if name.endswith(".so")]

        # 1. The two Java components that JNI resolves by name.
        classes = dex_classes(env, apk)
        record("DEX indexed", bool(classes), f"{len(dex_names)} dex file(s), {len(classes)} classes")
        for required in tuple(c for c in JNI_CLASSES if not c.endswith("/Native")) + (native_class,):
            record(f"dex contains {required}", required in classes, f"{len(classes)} classes indexed")

        # 2. Native payload: the single business library, no remdia SDK library.
        business = [name for name in so_names if Path(name).name == library]
        record(
            "business native library present",
            len(business) == 1 and business[0] == f"lib/{TARGET_ABI}/{library}",
            ", ".join(business) or "missing",
        )
        remdia_libs = [name for name in so_names if "remedia_sdk" in name or "remdia_sdk" in name]
        record("standalone remdia SDK library excluded", not remdia_libs, ", ".join(remdia_libs) or "absent")
        other = [name for name in so_names if name not in business and name not in remdia_libs]
        record(
            "remaining native libraries are system bridges",
            all("libandroidx.graphics.path.so" in name for name in other),
            ", ".join(sorted({Path(name).name for name in other})) or "none",
        )

        # 3. Use the SDK validator for ZIP alignment, including 16 KB libraries.
        zipalign = run(
            [str(env.tool("zipalign")), "-c", "-P", "16", "-v", "4", str(apk)], quiet=True, check=False, timeout=600
        )
        record(
            "zipalign -c -P 16 -v 4",
            zipalign.returncode == 0,
            (zipalign.stdout.splitlines() or ["no output"])[-1].strip(),
        )

        # 4. Sizes by category.
        sizes = {key: bucket["uncompressed"] for key, bucket in apk_size_buckets(entries).items()}
        print("  sizes (uncompressed): " + ", ".join(f"{key} {human(value)}" for key, value in sizes.items()))

    # 5. ELF alignment of the business library.
    with zipfile.ZipFile(apk) as archive:
        member = f"lib/{TARGET_ABI}/{library}"
        extracted = ROOT / "target" / "android-audit" / f"{Path(library).stem}-{args.profile}.so"
        extracted.parent.mkdir(parents=True, exist_ok=True)
        extracted.write_bytes(archive.read(member))
    readelf = run([env.llvm("llvm-readelf"), "-l", str(extracted)], quiet=True, check=False).stdout
    alignments = [int(value, 16) for value in re.findall(r"LOAD\s+0x[0-9a-f]+\s+0x[0-9a-f]+\s+0x[0-9a-f]+\s+\S+\s+\S+\s+(0x[0-9a-f]+)", readelf)]
    if not alignments:
        alignments = [int(value, 16) for value in re.findall(r"^\s*LOAD.*\s(0x[0-9a-f]+)\s*$", readelf, re.MULTILINE)]
    record(
        "ELF PT_LOAD segments are >= 16 KB aligned",
        bool(alignments) and all(value >= PAGE_SIZE_16K for value in alignments),
        ", ".join(hex(value) for value in alignments) or "no LOAD segment parsed",
    )

    # 6. Every Kotlin `external fun` has a matching exported JNI symbol.
    declared = set(re.findall(r"external fun (\w+)", kotlin))
    symbols = set(
        re.findall(
            r"Java_" + re.escape(namespace.replace(".", "_") + "_Native_") + r"(\w+)",
            run([env.llvm("llvm-nm"), "-D", "--defined-only", str(extracted)], quiet=True, check=False).stdout,
        )
    )
    missing = sorted(declared - symbols)
    record(
        "JNI exports cover the Kotlin declarations",
        not missing,
        f"{len(declared)} declared, {len(symbols)} exported" + (f", missing {missing}" if missing else ""),
    )
    extra = sorted(symbols - declared)
    if extra:
        print(f"  note: exported but not declared in Native.kt: {extra}")

    # 7. Keep rules for both JNI-referenced class sets.
    rules = (source_root / "android" / "app" / "proguard-rules.pro").read_text(encoding="utf-8")
    for pattern in ("dev.remdia", namespace + ".Native", "org.rustls.platformverifier"):
        record(f"proguard keeps {pattern}", pattern in rules, "present" if pattern in rules else "MISSING")

    report = {
        "apk": str(apk),
        "sha256": sha256(apk),
        "bytes": apk.stat().st_size,
        "profile": args.profile,
        "checks": checks,
        "sizes": sizes,
        "auditedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }
    out = ROOT / "target" / "android-audit" / f"apk-audit-{args.profile}-{now()}.json"
    out.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    failed = [check["name"] for check in checks if not check["ok"]]
    print(f"\nreport   {out}")
    print(f"sha256   {report['sha256']}")
    if failed:
        print(f"audit FAILED: {', '.join(failed)}", file=sys.stderr)
        return 1
    print(f"audit passed ({len(checks)} checks)")
    return 0


def command_measure(args: argparse.Namespace, env: Env) -> int:
    """Release baseline: build time, APK size buckets, R8 DEX keeps, memory.

    Fails loudly when the build fails, when the R8-kept JNI classes disappear
    from the DEX, or when the app does not start on the sampling device. Memory
    sampling is skipped (with a printed reason) when no device is attached.
    """
    profile = args.profile
    stamp = now()
    if args.cold:
        # Gradle caches and this crate's cargo artifacts dominate rebuild time;
        # this measures a partial rebuild, deliberately retaining the
        # dependency crates; it is not a clean-machine/CI cold build.
        for path in (ROOT / "android" / "build", ROOT / "android" / "app" / "build", ROOT / "android" / ".gradle"):
            shutil.rmtree(path, ignore_errors=True)
        run(
            ["cargo", "clean", "-p", "pordo-android", "--target", "aarch64-linux-android"],
            env=cross_env(env),
            check=False,
            quiet=True,
        )
        print("partial rebuild: removed Gradle build dirs and pordo-android artifacts; dependencies cached")

    command = [str(ROOT / "scripts" / "build-android.sh"), "--assemble"]
    if profile == "release":
        command.append("--release")
    log = ROOT / "target" / "android-audit" / f"measure-build-{profile}-{stamp}.log"
    print(f"building {profile} (log: {log})")
    started = time.time()
    result = run(command, env=env.build_env, log=log, check=False)
    build_seconds = time.time() - started
    if result.returncode != 0:
        raise ToolError(f"build failed; nothing measured (log: {log})")
    data = artifact(env, profile)
    apk = Path(data["path"])
    print(f"built {apk.name} in {build_seconds:.1f}s ({'cold' if args.cold else 'hot'} caches)")

    # Size buckets, compressed and uncompressed (audit reports uncompressed only).
    with zipfile.ZipFile(apk) as archive:
        sizes = apk_size_buckets(archive.infolist())
    print("  size buckets: " + ", ".join(f"{key} {human(value['compressed'])}" for key, value in sizes.items()))

    # R8 outcome: class count plus the JNI-referenced keeps that must survive.
    classes = dex_classes(env, apk)
    missing_keeps = sorted(set(JNI_CLASSES) - classes)
    print(f"  dex classes: {len(classes)}" + (f", MISSING keeps {missing_keeps}" if missing_keeps else ", JNI keeps present"))
    if missing_keeps:
        raise ToolError(f"R8 stripped JNI-referenced classes: {', '.join(missing_keeps)}")

    # Memory sampling on a device; the baseline is a cold start at rest.
    memory: dict | None = None
    try:
        serial = require_device(env, args.serial)
    except ToolError as error:
        serial = ""
        print(f"  memory sampling skipped: {error}")
    if serial:
        info = device_info(env, serial)
        for problem in check_device_capabilities(info):
            raise ToolError(problem)
        installed = adb(env, ["install", "--no-incremental", "-r", str(apk)], serial, timeout=900)
        if installed.returncode != 0 or "Success" not in installed.stdout:
            raise ToolError(f"install failed: {installed.stdout.strip()}")
        grant_permissions(env, serial, info)
        adb(env, ["shell", "am", "force-stop", PACKAGE], serial)
        launch = adb(env, ["shell", "am", "start", "-n", ACTIVITY], serial)
        if launch.returncode != 0:
            raise ToolError(f"am start failed: {launch.stdout.strip()}")
        pid = wait_for_process(env, serial, args.timeout)
        if not pid:
            raise ToolError("app process did not appear; cannot sample memory")
        time.sleep(5)
        samples = []
        for index in range(args.samples):
            dump = adb(env, ["shell", "dumpsys", "meminfo", PACKAGE], serial).stdout
            sample = {
                "totalPssKb": _first_int(dump, r"TOTAL PSS:\s+(\d+)", r"^\s*TOTAL\s+(\d+)", flags=re.MULTILINE),
                "totalRssKb": _first_int(dump, r"TOTAL RSS:\s+(\d+)"),
                "nativeHeapKb": _first_int(dump, r"Native Heap:\s+(\d+)", r"^\s*Native Heap\s+(\d+)", flags=re.MULTILINE),
                "javaHeapKb": _first_int(dump, r"Java Heap:\s+(\d+)", r"^\s*(?:Dalvik Heap|Java Heap)\s+(\d+)", flags=re.MULTILINE),
            }
            samples.append(sample)
            print(
                f"  sample {index + 1}: PSS {sample['totalPssKb']} KB"
                f" (native {sample['nativeHeapKb']} KB, java {sample['javaHeapKb']} KB)"
            )
            if index + 1 < args.samples:
                time.sleep(3)
        memory = {"device": info, "pid": pid, "samples": samples}
        adb(env, ["shell", "am", "force-stop", PACKAGE], serial)

    report = {
        "profile": profile,
        "cold": bool(args.cold),
        "buildSeconds": round(build_seconds, 1),
        "apk": str(apk),
        "sha256": data["sha256"],
        "bytes": int(data["bytes"]),
        "sizeBuckets": sizes,
        "dexClasses": len(classes),
        "memory": memory,
        "measuredAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }
    out = ROOT / "target" / "android-audit" / f"measure-{profile}-{now()}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\nreport   {out}")
    print(f"sha256   {data['sha256']}")
    print(f"measure: {profile} APK {human(int(data['bytes']))}, build {build_seconds:.1f}s, {len(classes)} dex classes")
    return 0


def _first_int(text: str, *patterns: str, flags: int = 0) -> int | None:
    """First matching group as int across alternative dumpsys output formats."""
    for pattern in patterns:
        match = re.search(pattern, text, flags)
        if match:
            return int(match.group(1))
    return None


COMMANDS = {
    "doctor": command_doctor,
    "check": command_check,
    "build": command_build,
    "run": command_run,
    "logs": command_logs,
    "smoke": command_smoke,
    "audit": command_audit,
    "measure": command_measure,
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=sorted(COMMANDS))
    parser.add_argument("--profile", choices=("debug", "release"), default=None, help="build profile (measure defaults to release)")
    parser.add_argument("--sdk", default=str(ROOT.parent / "remdia" / ".deps" / "android-sdk"))
    parser.add_argument("--ndk", default=None)
    parser.add_argument("--jdk", default=None)
    parser.add_argument("--adb", default=None, help="adb binary; the bundled 36.0.2 avoids ADB 37 host crashes")
    parser.add_argument("--serial", default=None)
    parser.add_argument("--timeout", type=int, default=120, help="deadline in seconds for launch (run) or instrumentation (smoke)")
    parser.add_argument("--clippy", action="store_true", help="check: run clippy for the Android target")
    parser.add_argument("--online", action="store_true", help="smoke: also run external-network cases")
    parser.add_argument(
        "--proxy",
        default=None,
        help="smoke --online: HTTP(S) proxy URL exported to the app so the native reqwest client can "
        "reach the source through a host tunnel (e.g. http://10.0.2.2:7890)",
    )
    parser.add_argument("--test", default=None, help="smoke: instrumentation class or class#method filter")
    parser.add_argument("--source-root", type=Path, default=ROOT, help="audit: source checkout matching the APK")
    parser.add_argument("--apk", default=None, help="audit: inspect a specific APK instead of the published artifact")
    parser.add_argument("--allow-stale", action="store_true", help="run/smoke: install an APK older than the sources")
    parser.add_argument("--cold", action="store_true", help="measure: drop Gradle and pordo-android cargo caches before building")
    parser.add_argument("--samples", type=int, default=3, help="measure: dumpsys meminfo samples after launch")
    args = parser.parse_args(argv)
    if args.profile is None:
        args.profile = "release" if args.command == "measure" else "debug"

    try:
        env = resolve_env(args)
        return COMMANDS[args.command](args, env)
    except ToolError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
