"""DMG creation recovers from contention and never publishes incomplete images."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location('macos_package', SCRIPTS / 'package-macos.py')
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)


class MacOSPackageTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory(dir=SCRIPTS.parent / 'target')
        self.addCleanup(self.work.cleanup)
        self.root = Path(self.work.name)
        self.stage = self.root / 'stage'
        self.stage.mkdir()
        self.destination = self.root / 'Pordo.dmg'
        self.destination.write_bytes(b'previous image')

    def invoke(self, outcomes):
        def create(command, **kwargs):
            self.assertEqual(self.destination.read_bytes(), b'previous image')
            image = Path(command[-1])
            self.assertFalse(image.exists(), 'Partial image must be removed before retry')
            self.assertFalse(image.is_relative_to(self.stage))
            outcome = next(outcomes)
            image.write_bytes(b'partial' if outcome else b'complete')
            if outcome:
                raise subprocess.CalledProcessError(1, command, output=outcome)
            return 'created\n'

        with patch.object(package.subprocess, 'check_output', side_effect=create) as process, \
                patch.object(package.time, 'sleep') as sleep, \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            try:
                package.create_dmg(self.stage, self.destination)
            finally:
                self.attempts = process.call_count
                self.delays = [call.args[0] for call in sleep.call_args_list]
                self.assertFalse(list(self.root.glob('.dmg-*')))

    def test_success_replaces_previous_image(self):
        self.invoke(iter([None]))
        self.assertEqual(self.destination.read_bytes(), b'complete')
        self.assertEqual(self.attempts, 1)
        self.assertEqual(self.delays, [])

    def test_busy_failure_cleans_partial_image_then_recovers(self):
        self.invoke(iter(['hdiutil: create failed - Resource busy', None]))
        self.assertEqual(self.destination.read_bytes(), b'complete')
        self.assertEqual(self.attempts, 2)
        self.assertEqual(self.delays, [5])

    def test_repeated_contention_fails_after_three_attempts(self):
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.invoke(iter(['hdiutil: create failed - Resource busy'] * 3))
        self.assertIn('Resource busy', error.exception.output)
        self.assertEqual(self.destination.read_bytes(), b'previous image')
        self.assertEqual(self.attempts, 3)
        self.assertEqual(self.delays, [5, 10])

    def test_other_errors_fail_immediately(self):
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.invoke(iter(['hdiutil: create failed - Permission denied']))
        self.assertIn('Permission denied', error.exception.output)
        self.assertEqual(self.destination.read_bytes(), b'previous image')
        self.assertEqual(self.attempts, 1)
        self.assertEqual(self.delays, [])


if __name__ == '__main__':
    unittest.main()
