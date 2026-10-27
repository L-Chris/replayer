#!/usr/bin/env python3
"""Package a native build as a relocatable .app, ZIP and drag-to-install DMG."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
from macos_bundle import architecture, embed_libraries, run, sign_bundle, verify_bundle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/release/replayer'))
    parser.add_argument('--output-directory', type=Path, default=Path('dist'))
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('macOS packaging requires Apple command-line tools on macOS.')
    root = Path(__file__).resolve().parents[1]
    os.chdir(root)
    binary = args.binary.resolve(strict=True)
    prefix = Path(os.environ['FFMPEG_DIR']).resolve(strict=True)
    listing = run(prefix / 'bin/ffmpeg', '-hide_banner', '-encoders')
    encoders = re.findall(r'^\s*[VAS][.A-Z]{5}\s+([a-z0-9_-]+)\s', listing, re.MULTILINE)
    if encoders != ['libmp3lame']:
        raise ValueError('Packages require only libmp3lame encoding; run scripts/setup-macos.sh first.')
    metadata = json.loads(run('cargo', 'metadata', '--no-deps', '--locked', '--format-version', '1'))
    version = next(p['version'] for p in metadata['packages'] if p['name'] == 'replayer')
    arch = architecture(binary)
    rust_arch = 'aarch64' if arch == 'arm64' else arch
    minimum = os.environ.get('MACOSX_DEPLOYMENT_TARGET', '15.0')
    output = args.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=True)
    stage_root = root / 'target' / 'macos-package'
    stage_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=stage_root) as temporary:
        stage = Path(temporary)
        app = stage / 'replayer.app'
        contents = app / 'Contents'
        (contents / 'MacOS').mkdir(parents=True)
        resources = contents / 'Resources'
        resources.mkdir()
        executable = contents / 'MacOS' / 'replayer'
        shutil.copy2(binary, executable)
        executable.chmod(0o755)
        libraries = embed_libraries(executable, contents / 'Frameworks', prefix)
        shutil.copy2(root / 'assets/branding/replayer.icns', resources)
        # Whitelist package resources; never copy settings, media or the working tree.
        shutil.copytree(prefix / 'share/replayer/licenses', resources / 'licenses')
        shutil.copytree(prefix / 'share/replayer/sources', resources / 'sources')
        shutil.copy2(root / 'docs/third-party.md', resources / 'THIRD_PARTY.md')
        shutil.copy2(root / 'README.md', resources)
        (resources / 'docs').mkdir()
        for name in ('guide.md', 'third-party.md'):
            shutil.copy2(root / 'docs' / name, resources / 'docs' / name)
        shutil.copytree(root / 'licenses', resources / 'licenses/replayer')
        with (contents / 'Info.plist').open('wb') as file:
            plistlib.dump({
                'CFBundleName': 'replayer', 'CFBundleDisplayName': 'replayer',
                'CFBundleIdentifier': 'io.github.l-chris.replayer',
                'CFBundleExecutable': 'replayer', 'CFBundlePackageType': 'APPL',
                'CFBundleShortVersionString': version, 'CFBundleVersion': version,
                'CFBundleIconFile': 'replayer.icns', 'CFBundleSupportedPlatforms': ['MacOSX'],
                'LSApplicationCategoryType': 'public.app-category.entertainment',
                'LSMinimumSystemVersion': minimum, 'NSHighResolutionCapable': True,
                'NSSupportsAutomaticGraphicsSwitching': True,
            }, file)
        sign_bundle(app, libraries)
        verify_bundle(app, arch, version)
        reported = subprocess.check_output([str(executable), '--version'], text=True, timeout=15).strip()
        assert reported == 'replayer ' + version, ('Executable version mismatch', reported)
        profile = os.environ.get('MACOS_NOTARY_PROFILE')
        if profile:
            submission = stage / 'notarization.zip'
            run('ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', app, submission)
            run('xcrun', 'notarytool', 'submit', submission, '--keychain-profile', profile, '--wait')
            run('xcrun', 'stapler', 'staple', app)
        name = f'replayer-{version}-macos-{rust_arch}'
        archive = output / (name + '.zip')
        dmg = output / (name + '.dmg')
        run('ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', app, archive)
        (stage / 'Applications').symlink_to('/Applications')
        # Only the app and Applications link go in the image (not the notary submission).
        if profile:
            submission.unlink()
        run('hdiutil', 'create', '-ov', '-format', 'UDZO', '-fs', 'HFS+',
            '-volname', 'replayer', '-srcfolder', stage, dmg)
        if profile:
            run('xcrun', 'notarytool', 'submit', dmg, '--keychain-profile', profile, '--wait')
            run('xcrun', 'stapler', 'staple', dmg)
        manifest = output / f'SHA256SUMS-macos-{rust_arch}.txt'
        manifest.write_text(''.join(hashlib.sha256(path.read_bytes()).hexdigest() + '  ' + path.name + '\n'
                                    for path in (archive, dmg)), encoding='ascii')
        print(f'Packaged replayer {version} ({arch}, macOS {minimum}+): {archive}, {dmg}')


if __name__ == '__main__':
    main()
