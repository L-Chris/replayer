"""Mach-O dependency relocation and verification shared by packaging and smoke tests."""
from pathlib import Path
import os
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
    """Copy only the dependency closure; fail rather than ship unrelated host libraries."""
    prefix = prefix.resolve()
    framework_dir.mkdir()
    sources = {}
    names = {}

    def resolve(dependency, loader):
        if dependency.startswith('@loader_path/'):
            candidate = loader.parent / dependency[len('@loader_path/'):]
        elif dependency.startswith('@executable_path/'):
            candidate = executable.parent / dependency[len('@executable_path/'):]
        elif dependency.startswith('@rpath/'):
            candidate = prefix / 'lib' / dependency[len('@rpath/'):]
        else:
            candidate = Path(dependency)
        candidate = candidate.resolve(strict=True)
        if not candidate.is_relative_to(prefix / 'lib'):
            raise ValueError(f'Non-system dependency outside the pinned runtime: {candidate}. '
                             'Build using scripts/build-macos.sh.')
        return candidate

    def visit(source, destination):
        for dependency in dependencies(source):
            if system_library(dependency):
                continue
            library = resolve(dependency, source)
            if library not in sources:
                if library.name in names and names[library.name] != library:
                    raise ValueError(f'Conflicting library names: {library.name}')
                names[library.name] = library
                copied = framework_dir / library.name
                sources[library] = copied
                shutil.copy2(library, copied)
                copied.chmod(0o755)
                visit(library, copied)
            run('install_name_tool', '-change', dependency, '@rpath/' + library.name, destination)

    # The executable was copied into its final bundle location before this call.
    visit(executable, executable)
    for library in sources.values():
        run('install_name_tool', '-id', '@rpath/' + library.name, library)
    for binary in [executable, *sources.values()]:
        for path in rpaths(binary):
            run('install_name_tool', '-delete_rpath', path, binary)
    run('install_name_tool', '-add_rpath', '@executable_path/../Frameworks', executable)
    return list(sources.values())


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
    assert {file.name for file in (contents / 'Resources/sources').iterdir()} == {
        'ffmpeg-9.0.2.tar.xz', 'lame-4.0.tar.gz', 'setup-macos.sh',
        'ffmpeg-playback-options.sh', 'verify-ffmpeg-config.py', 'BUILD.txt'
    }, 'Package sources must contain only the public build recipe and matching source archives'
    executable = contents / 'MacOS' / 'replayer'
    framework_dir = contents / 'Frameworks'
    libraries = list(framework_dir.glob('*.dylib'))
    assert libraries, 'Missing bundled FFmpeg runtime'
    for binary in [executable, *libraries]:
        assert architecture(binary) == expected_arch, binary
        assert minimum_version(binary) <= minimum, f'{binary} requires a newer macOS than Info.plist'
        for dependency in dependencies(binary):
            if not system_library(dependency):
                assert dependency.startswith('@rpath/'), (binary, dependency)
                assert (framework_dir / dependency[len('@rpath/'):]).is_file(), (binary, dependency)
        assert all(path == '@executable_path/../Frameworks' for path in rpaths(binary)), binary
    run('codesign', '--verify', '--deep', '--strict', app)
    return info
