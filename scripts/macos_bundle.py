"""Mach-O dependency relocation and verification shared by packaging and smoke tests."""
from pathlib import Path
import os
import json
import plistlib
import re
import shutil
import subprocess


def run(*args, timeout=None):
    return subprocess.check_output([str(arg) for arg in args], text=True, timeout=timeout).strip()


def system_library(path):
    return path.startswith(('/usr/lib/', '/System/Library/'))


def install_id(path):
    lines = run('otool', '-D', path).splitlines()
    return lines[1].strip() if len(lines) > 1 else None


def dependencies(path):
    own_id = install_id(path)
    return [line.strip().split(' (compatibility version', 1)[0]
            for line in run('otool', '-L', path).splitlines()[1:]
            if line.strip().split(' (compatibility version', 1)[0] != own_id]


def rpaths(path):
    return re.findall(r'cmd LC_RPATH\s+cmdsize \d+\s+path (.*?) \(offset', run('otool', '-l', path))


def architecture(path):
    value = run('lipo', '-archs', path)
    if value not in ('arm64', 'x86_64'):
        raise ValueError(f'Expected a single supported architecture: {path}: {value}')
    return value


def minimum_version(path):
    commands = run('otool', '-l', path)
    match = re.search(r'\bminos ([\d.]+)', commands)
    if not match:
        match = re.search(r'cmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version ([\d.]+)', commands)
    if not match:
        raise ValueError(f'Missing macOS deployment target: {path}')
    return tuple((list(map(int, match[1].split('.'))) + [0, 0])[:3])


def embed_libraries(executable, framework_dir, prefix):
    """Explicitly embed dlopen packs, which are absent from the host import list."""
    plugins = prefix / 'plugins'
    destination = framework_dir / 'remdia'
    destination.mkdir(parents=True)
    libraries = []
    for name in ('native', 'macos'):
        manifest = plugins / (name + '.json')
        data = json.loads(manifest.read_text())
        library = data['library']
        if Path(library).name != library or library != f'libremdia_backend_{name}.dylib':
            raise ValueError(f'Unexpected component filename: {library}')
        shutil.copy2(manifest, destination / manifest.name)
        copied = destination / library
        shutil.copy2(plugins / library, copied)
        copied.chmod(0o755)
        libraries.append(copied)
        run('install_name_tool', '-id', '@rpath/remdia/' + library, copied)
    for binary in [executable, *libraries]:
        for dependency in dependencies(binary):
            if not system_library(dependency):
                raise ValueError(f'Unexpected non-system import: {binary}: {dependency}')
        for path in rpaths(binary):
            run('install_name_tool', '-delete_rpath', path, binary)
    run('install_name_tool', '-add_rpath', '@executable_path/../Frameworks', executable)
    return libraries


def sign_bundle(app, libraries):
    identity = os.environ.get('MACOS_CODESIGN_IDENTITY', '-')
    flags = ['--force', '--sign', identity]
    if identity != '-':
        flags += ['--options', 'runtime', '--timestamp']
    for library in libraries:
        run('codesign', *flags, library)
    run('codesign', *flags, app)


def verify_bundle(app, expected_arch, expected_version=None):
    contents = app / 'Contents'
    with (contents / 'Info.plist').open('rb') as file:
        info = plistlib.load(file)
    assert info['CFBundleIdentifier'] == 'io.github.l-chris.replayer'
    assert info['CFBundleExecutable'] == 'replayer'
    assert info['CFBundlePackageType'] == 'APPL'
    if expected_version:
        assert info['CFBundleShortVersionString'] == expected_version
    minimum = tuple((list(map(int, info['LSMinimumSystemVersion'].split('.'))) + [0, 0])[:3])
    icon = contents / 'Resources' / info['CFBundleIconFile']
    assert icon.read_bytes()[:4] == b'icns', 'Missing macOS application icon'
    notices = contents / 'Resources/remdia'
    revision = (notices / 'REVISION').read_text().strip()
    assert re.fullmatch('[a-f0-9]{40}', revision), 'Invalid SDK revision'
    for name in ('THIRD_PARTY.md', 'LICENSE', 'BUILD.txt'):
        assert (notices / name).is_file(), 'Missing SDK source/rebuild notice: ' + name
    assert any((notices / 'licenses').iterdir()), 'Missing codec licenses'
    executable = contents / 'MacOS/replayer'
    framework_dir = contents / 'Frameworks'
    plugins = framework_dir / 'remdia'
    expected_files = {'native.json', 'macos.json', 'libremdia_backend_native.dylib',
                      'libremdia_backend_macos.dylib'}
    assert {p.name for p in plugins.iterdir()} == expected_files, 'Unexpected component pack'
    assert {p.name for p in framework_dir.iterdir()} == {'remdia'}
    for name in ('native', 'macos'):
        manifest = json.loads((plugins / (name + '.json')).read_text())
        assert manifest['id'] == name and manifest['os'] == 'macos'
        assert manifest['abi_major'] == 2 and manifest.get('abi_minor', 0) <= 1
        assert manifest['arch'] == ('aarch64' if expected_arch == 'arm64' else expected_arch)
        assert manifest['library'] == f'libremdia_backend_{name}.dylib'
    forbidden = re.compile(r'(ffmpeg|avcodec|avformat|avutil|swresample|swscale)', re.I)
    for path in contents.rglob('*'):
        assert not forbidden.search(path.name), 'Unexpected legacy runtime: ' + str(path)
    for binary in [executable, *plugins.glob('*.dylib')]:
        assert architecture(binary) == expected_arch, binary
        assert minimum_version(binary) <= minimum, f'{binary} requires a newer macOS than Info.plist'
        for dependency in dependencies(binary):
            assert system_library(dependency), (binary, dependency)
            assert not forbidden.search(dependency), (binary, dependency)
        assert all(path == '@executable_path/../Frameworks' for path in rpaths(binary)), binary
    run('codesign', '--verify', '--deep', '--strict', app)
    return info
