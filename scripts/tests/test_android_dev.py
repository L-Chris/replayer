"""Host-only regressions for the Android development runner; no SDK/device needed."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import time
import unittest


SPEC = importlib.util.spec_from_file_location(
    "android_dev", Path(__file__).resolve().parents[1] / "android-dev.py"
)
dev = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = dev
SPEC.loader.exec_module(dev)


class AndroidDevTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        (dev.ROOT / "target").mkdir(exist_ok=True)

    def test_silent_process_obeys_deadline_and_keeps_partial_log(self):
        with tempfile.TemporaryDirectory(dir=dev.ROOT / "target") as directory:
            log = Path(directory) / "timeout.log"
            started = time.monotonic()
            with self.assertRaisesRegex(dev.ToolError, "timed out"):
                dev.run(
                    [sys.executable, "-u", "-c", "import time; print('started'); time.sleep(10)"],
                    quiet=True,
                    timeout=0.2,
                    log=log,
                )
            self.assertLess(time.monotonic() - started, 3)
            self.assertIn("started", log.read_text())

    def test_failed_command_keeps_combined_output(self):
        with tempfile.TemporaryDirectory(dir=dev.ROOT / "target") as directory:
            log = Path(directory) / "failure.log"
            result = dev.run(
                [sys.executable, "-c", "import sys; print('failed', file=sys.stderr); sys.exit(7)"],
                quiet=True,
                check=False,
                log=log,
            )
            self.assertEqual(7, result.returncode)
            self.assertEqual(result.stdout, log.read_text())
            self.assertIn("failed", result.stdout)

    def test_explicit_ndk_reaches_the_build_script(self):
        with tempfile.TemporaryDirectory(dir=dev.ROOT / "target") as directory:
            root = Path(directory)
            env = dev.Env(sdk=root / "sdk", ndk=root / "custom-ndk", jdk=root / "jdk", adb=root / "adb")
            cross = dev.cross_env(env)
            self.assertEqual(str(env.ndk), cross["ANDROID_NDK_HOME"])
            self.assertTrue(cross["CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"].startswith(str(env.ndk)))

    def test_runner_cannot_pass_with_missing_cases_or_an_error_exit(self):
        output = "\n".join([
            "INSTRUMENTATION_STATUS: class=Example",
            "INSTRUMENTATION_STATUS: test=onlyCase",
            "INSTRUMENTATION_STATUS: numtests=2",
            "INSTRUMENTATION_STATUS_CODE: 0",
            "INSTRUMENTATION_CODE: -1",
        ])
        self.assertFalse(dev.parse_instrumentation(output)["complete"])
        output = output.replace("numtests=2", "numtests=1")
        self.assertTrue(dev.parse_instrumentation(output)["complete"])
        self.assertFalse(dev.parse_instrumentation(output.replace("CODE: -1", "CODE: 0"))["complete"])


if __name__ == "__main__":
    unittest.main()
