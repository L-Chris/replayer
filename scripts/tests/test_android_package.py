"""Release contract tests: reject wrong identities and preserve application payloads."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SPEC = importlib.util.spec_from_file_location("android_package", Path(__file__).resolve().parents[1] / "package-android.py")
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)


class AndroidPackageTests(unittest.TestCase):
    def test_manifest_must_match_release_and_supported_devices(self):
        metadata = "package: name='dev.replayer' versionCode='2' versionName='0.5.1'\nsdkVersion:'28'\nnative-code: 'arm64-v8a'\n"
        package.verify_metadata(metadata, "0.5.1")
        for value in [metadata.replace("0.5.1", "0.5.0"), metadata.replace("dev.replayer", "dev.other"),
                      metadata + "application-debuggable\n", metadata.replace("'28'", "'27'"),
                      metadata.replace("'arm64-v8a'", "'arm64-v8a' 'x86_64'")]:
            with self.assertRaises(ValueError):
                package.verify_metadata(value, "0.5.1")

    def test_certificate_is_pinned_and_required_schemes_are_present(self):
        fingerprint = "a" * 64
        output = f"Signer #1 certificate SHA-256 digest: {fingerprint}\nVerified using v2 scheme (APK Signature Scheme v2): true\nVerified using v3 scheme (APK Signature Scheme v3): true\n"
        for scheme in [2, 3]:
            package.verify_certificate(output, fingerprint, scheme)
        for value in [output.replace(fingerprint, "b" * 64), output.replace("v2): true", "v2): false"),
                      output.replace("v3): true", "v3): false"), output + f"Signer #2 certificate SHA-256 digest: {fingerprint}\n"]:
            with self.assertRaises(ValueError):
                package.verify_certificate(value, fingerprint, 2)
                package.verify_certificate(value, fingerprint, 3)

    def test_signatures_are_excluded_but_other_metadata_is_part_of_payload(self):
        with tempfile.TemporaryDirectory(dir=package.ROOT / "target") as work:
            first, second = Path(work) / "first.apk", Path(work) / "second.apk"
            for apk, signature in [(first, b"old"), (second, b"new")]:
                with zipfile.ZipFile(apk, "w") as archive:
                    archive.writestr("classes.dex", b"code")
                    archive.writestr("META-INF/NOTICE.txt", b"license")
                    archive.writestr("META-INF/NOTICE.MF", b"application metadata")
                    archive.writestr("META-INF/CERT.RSA", signature)
            self.assertEqual(package.payload(first), package.payload(second))
            self.assertIn("META-INF/NOTICE.MF", package.payload(first))
            with zipfile.ZipFile(second, "a") as archive:
                archive.writestr("lib/arm64-v8a/replaced.so", b"unexpected")
            self.assertNotEqual(package.payload(first), package.payload(second))

    def test_tool_failures_do_not_echo_signing_secrets(self):
        import subprocess
        failure = subprocess.CompletedProcess([], 1, stdout="private secret", stderr="private secret")
        with patch.object(package.subprocess, "run", return_value=failure):
            with self.assertRaisesRegex(RuntimeError, "failed") as error:
                package.run(["apksigner", "sign"])
            self.assertNotIn("private secret", str(error.exception))


if __name__ == "__main__":
    unittest.main()
