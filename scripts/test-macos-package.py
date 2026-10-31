#!/usr/bin/env python3
"""Verify both archives, then play media from the relocated app without developer paths."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import platform
import tempfile
from macos_bundle import run, verify_bundle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, default=Path('dist'))
    parser.add_argument('--skip-dmg-mount', action='store_true',
                        help='Verify DMG integrity without mounting it; still fully test the ZIP app.')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    os.chdir(root)
    directory = args.directory.resolve()
    fixtures = Path(os.environ.get('PORDO_NATIVE_FIXTURES', root / '.deps/remdia-source/tests/fixtures'))
    media = (fixtures / 'av.mp4').resolve(strict=True)
    arch = platform.machine()
    rust_arch = 'aarch64' if arch == 'arm64' else arch
    manifest = directory / f'SHA256SUMS-macos-{rust_arch}.txt'
    files = []
    for line in manifest.read_text(encoding='ascii').splitlines():
        digest, name = line.split('  ', 1)
        assert Path(name).name == name and name.endswith(('.zip', '.dmg')), name
        path = directory / name
        assert hashlib.sha256(path.read_bytes()).hexdigest() == digest, name
        files.append(path)
    assert len(files) == 2 and {file.suffix for file in files} == {'.zip', '.dmg'}
    work = root / 'target' / 'macos-smoke'
    work.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=work) as temporary:
        temporary = Path(temporary)
        for archive in files:
            if archive.suffix == '.dmg' and args.skip_dmg_mount:
                run('hdiutil', 'verify', archive, timeout=60)
                print('PASS:', archive.name, 'checksum verified; mount test explicitly skipped')
                continue
            destination = temporary / (archive.suffix[1:] + ' relocated app')
            destination.mkdir()
            if archive.suffix == '.zip':
                run('ditto', '-x', '-k', archive, destination)
            else:
                mount = temporary / 'mounted image'
                run('hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', mount, archive, timeout=60)
                try:
                    assert (mount / 'Applications').is_symlink()
                    run('ditto', mount / 'Pordo.app', destination / 'Pordo.app')
                finally:
                    run('hdiutil', 'detach', mount)
            app = destination / 'Pordo.app'
            info = verify_bundle(app, arch)
            executable = app / 'Contents/MacOS/pordo'
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith(('DYLD_', 'FFMPEG_', 'LIBCLANG_', 'PKG_CONFIG_', 'REMDIA_'))}
            env.update(PATH='/usr/bin:/bin:/usr/sbin:/sbin', PORDO_SOFTWARE='1',
                       PORDO_SETTINGS_FILE=str(temporary / 'settings.json'))
            result = subprocess.check_output([str(executable), '--version'], env=env, text=True, timeout=15)
            assert result.strip() == 'pordo ' + info['CFBundleShortVersionString']
            subprocess.run([str(executable), '--selftest', str(media)], env=env,
                           cwd=destination, check=True, timeout=90)
            print('PASS:', archive.name, 'signature, architecture, icon, relocated libraries and playback')


if __name__ == '__main__':
    main()
