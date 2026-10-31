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
    parser.add_argument('--binary', type=Path, default=Path('target/release/pordo'))
    parser.add_argument('--output-directory', type=Path, default=Path('dist'))
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('macOS packaging requires Apple command-line tools on macOS.')
    root = Path(__file__).resolve().parents[1]
    os.chdir(root)
    binary = args.binary.resolve(strict=True)
    prefix = Path(os.environ.get('REMDIA_RUNTIME_DIR', '.deps/remdia-source/dist/remdia')).resolve(strict=True)
    cargo = (root / 'Cargo.toml').read_text()
    revision = re.search(r'^remdia\s*=\s*\{[^}]*rev\s*=\s*"([a-f0-9]{40})"', cargo, re.M)[1]
    if (prefix / 'REVISION').read_text().strip() != revision:
        raise ValueError('Runtime revision differs from Cargo.toml; run scripts/setup-macos.sh.')
    metadata = json.loads(run('cargo', 'metadata', '--no-deps', '--locked', '--format-version', '1'))
    version = next(p['version'] for p in metadata['packages'] if p['name'] == 'pordo')
    arch = architecture(binary)
    rust_arch = 'aarch64' if arch == 'arm64' else arch
    tree = run('cargo', 'tree', '--locked', '--target', rust_arch + '-apple-darwin', '-e', 'normal', '--prefix', 'none')
    assert not re.search(r'^ffmpeg', tree, re.M | re.I), 'Cargo dependencies still contain FFmpeg'
    minimum = os.environ.get('MACOSX_DEPLOYMENT_TARGET', '15.0')
    output = args.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=True)
    stage_root = root / 'target' / 'macos-package'
    stage_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=stage_root) as temporary:
        stage = Path(temporary)
        app = stage / 'Pordo.app'
        contents = app / 'Contents'
        (contents / 'MacOS').mkdir(parents=True)
        resources = contents / 'Resources'
        resources.mkdir()
        executable = contents / 'MacOS' / 'pordo'
        shutil.copy2(binary, executable)
        executable.chmod(0o755)
        libraries = embed_libraries(executable, contents / 'Frameworks', prefix)
        shutil.copy2(root / 'assets/branding/pordo.icns', resources)
        # Whitelist package resources; never copy settings, media or the working tree.
        sdk_notices = resources / 'remdia'
        sdk_notices.mkdir(exist_ok=True)
        for name in ('REVISION', 'THIRD_PARTY.md', 'LICENSE'):
            shutil.copy2(prefix / name, sdk_notices / name)
        shutil.copytree(prefix / 'licenses', sdk_notices / 'licenses')
        (sdk_notices / 'BUILD.txt').write_text(
            f'SDK source: https://github.com/PordoTech/remdia/tree/{revision}\n'
            f'Source archive: https://github.com/PordoTech/remdia/archive/{revision}.tar.gz\n'
            'Install Rust stable, CMake and Apple command-line tools. On matching macOS 15+ hardware:\n'
            f'git clone https://github.com/PordoTech/remdia.git\ncd remdia\ngit checkout {revision}\n'
            f'./scripts/build-macos.sh --release --target {rust_arch}-apple-darwin\n'
            'Built-in remdia system components are linked into the application.\n'
            'Cargo.lock fixes the exact source distributions and checksums, including the separate LAME extension.\n'
            'Replace the matching dylib in Contents/Frameworks/remdia with an ABI-compatible rebuild,\n'
            'then codesign --force --sign - LIBRARY and codesign --force --sign - Pordo.app.\n',
            encoding='utf-8')
        shutil.copy2(root / 'docs/third-party.md', resources / 'THIRD_PARTY.md')
        shutil.copy2(root / 'README.md', resources)
        (resources / 'docs').mkdir()
        for name in ('guide.md', 'third-party.md'):
            shutil.copy2(root / 'docs' / name, resources / 'docs' / name)
        shutil.copytree(root / 'licenses', resources / 'licenses')
        with (contents / 'Info.plist').open('wb') as file:
            plistlib.dump({
                'CFBundleName': 'Pordo', 'CFBundleDisplayName': 'Pordo',
                'CFBundleIdentifier': 'io.github.l-chris.replayer',
                'CFBundleExecutable': 'pordo', 'CFBundlePackageType': 'APPL',
                'CFBundleShortVersionString': version, 'CFBundleVersion': version,
                'CFBundleIconFile': 'pordo.icns', 'CFBundleSupportedPlatforms': ['MacOSX'],
                'LSApplicationCategoryType': 'public.app-category.entertainment',
                'LSMinimumSystemVersion': minimum, 'NSHighResolutionCapable': True,
                'NSSupportsAutomaticGraphicsSwitching': True,
            }, file)
        sign_bundle(app, libraries)
        verify_bundle(app, arch, version)
        reported = subprocess.check_output([str(executable), '--version'], text=True, timeout=15).strip()
        assert reported == 'pordo ' + version, ('Executable version mismatch', reported)
        profile = os.environ.get('MACOS_NOTARY_PROFILE')
        if profile:
            submission = stage / 'notarization.zip'
            run('ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', app, submission)
            run('xcrun', 'notarytool', 'submit', submission, '--keychain-profile', profile, '--wait')
            run('xcrun', 'stapler', 'staple', app)
        name = f'pordo-{version}-macos-{rust_arch}'
        archive = output / (name + '.zip')
        dmg = output / (name + '.dmg')
        run('ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', app, archive)
        (stage / 'Applications').symlink_to('/Applications')
        # Only the app and Applications link go in the image (not the notary submission).
        if profile:
            submission.unlink()
        run('hdiutil', 'create', '-ov', '-format', 'UDZO', '-fs', 'HFS+',
            '-volname', 'Pordo', '-srcfolder', stage, dmg)
        if profile:
            run('xcrun', 'notarytool', 'submit', dmg, '--keychain-profile', profile, '--wait')
            run('xcrun', 'stapler', 'staple', dmg)
        manifest = output / f'SHA256SUMS-macos-{rust_arch}.txt'
        manifest.write_text(''.join(hashlib.sha256(path.read_bytes()).hexdigest() + '  ' + path.name + '\n'
                                    for path in (archive, dmg)), encoding='ascii')
        print(f'Packaged pordo {version} ({arch}, macOS {minimum}+): {archive}, {dmg}')


if __name__ == '__main__':
    main()
