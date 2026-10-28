#!/usr/bin/env python3
"""Validate Windows Cargo dependencies and inspect PE imports without LLVM."""
import argparse
import re
import struct
import subprocess
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument('--directory', required=True)
a = p.parse_args()
tree = subprocess.check_output(['cargo', 'tree', '--locked', '--target', 'x86_64-pc-windows-msvc', '-e', 'normal', '--prefix', 'none'], text=True)
assert not re.search(r'^ffmpeg', tree, re.M | re.I), 'Windows dependency tree still contains FFmpeg'
assert not re.search(r'^image-webp ', tree, re.M), 'Windows dependency tree still contains image-webp'

def imports(path):
    data = path.read_bytes()
    def u16(n): return struct.unpack_from('<H', data, n)[0]
    def u32(n): return struct.unpack_from('<I', data, n)[0]
    pe = u32(0x3c)
    assert data[pe:pe + 4] == b'PE\0\0', path
    optional = pe + 24
    directory = optional + (112 if u16(optional) == 0x20b else 96)
    section = optional + u16(pe + 20)
    sections = []
    for i in range(u16(pe + 6)):
        base = section + i * 40
        sections.append((u32(base + 12), max(u32(base + 8), u32(base + 16)), u32(base + 20)))
    def offset(rva):
        for address, size, file_at in sections:
            if address <= rva < address + size:
                return file_at + rva - address
        raise ValueError(f'Unmapped import RVA in {path}')
    rva = u32(directory + 8)
    if not rva: return []
    at = offset(rva)
    result = []
    while any(data[at:at + 20]):
        name = offset(u32(at + 12))
        end = data.index(b'\0', name, name + 1024)
        result.append(data[name:end].decode('ascii'))
        at += 20
    return result

root = Path(a.directory)
assert not (root / 'remdia-licenses/Opus-COPYING.txt').exists(), 'Stale SDK license from a cached runtime'
for path in root.rglob('*'):
    assert not re.search(r'(ffmpeg|avcodec|avformat|avutil|swresample|swscale|image[_-]webp)', path.name, re.I), path
    if path.suffix.lower() in ('.exe', '.dll'):
        linked = imports(path)
        assert not any(re.search(r'(avcodec|avformat|avutil|swresample|swscale|vcruntime|msvcp|msvcr\d)', item, re.I) for item in linked), (path, linked)
for name in ('native', 'windows', 'image', 'system_audio', 'system_demux', 'fallback_demux', 'fallback_vorbis', 'fallback_mp3_lame'):
    assert (root / 'plugins' / f'remdia_backend_{name}.dll').is_file(), f'Missing {name} pack'
print('Windows Cargo dependencies and PE imports are FFmpeg-free; component CRT is static')
