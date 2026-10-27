#!/usr/bin/env python3
"""Reject encoder bloat or decoder regressions in a custom FFmpeg runtime."""
import argparse
from pathlib import Path
import re


def enabled(path, suffix):
    return set(re.findall(r'^#define CONFIG_(\w+_' + suffix + r') 1$',
                          path.read_text(), re.MULTILINE))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('config', type=Path)
    parser.add_argument('--baseline', type=Path)
    args = parser.parse_args()
    expected = {'LIBMP3LAME_ENCODER'}
    encoders = enabled(args.config, 'ENCODER')
    if encoders != expected:
        raise SystemExit(f'Unexpected encoder configuration: {sorted(encoders ^ expected)}')
    decoders = enabled(args.config, 'DECODER')
    required = {'H264_DECODER', 'HEVC_DECODER', 'VP9_DECODER', 'AV1_DECODER',
                'AAC_DECODER', 'FLAC_DECODER', 'MP3FLOAT_DECODER'}
    if not required <= decoders:
        raise SystemExit(f'Missing playback decoders: {sorted(required - decoders)}')
    if args.baseline:
        for suffix in ('DECODER', 'DEMUXER', 'PARSER', 'HWACCEL', 'PROTOCOL'):
            lost = enabled(args.baseline, suffix) - enabled(args.config, suffix)
            if lost:
                raise SystemExit(f'Playback capabilities removed: {sorted(lost)}')
    print(f'PASS: {len(decoders)} decoders retained; only libmp3lame encoding enabled')


if __name__ == '__main__':
    main()
