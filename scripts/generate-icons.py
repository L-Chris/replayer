"""Rebuild icon containers: uv run --with pillow==12.1.1 python scripts/generate-icons.py.

Only resize/encode the approved logo; preserve its design and transparency.
Generated files are committed, so Windows CI does not require Python/Pillow.
"""
from pathlib import Path
from PIL import Image

root = Path(__file__).resolve().parents[1] / 'assets' / 'branding'
with Image.open(root / 'replayer-logo.png') as source:
    rgba = source.convert('RGBA')
    rgba.resize((256, 256), Image.Resampling.LANCZOS).save(root / 'replayer-icon-256.png')
    rgba.save(root / 'replayer.ico', format='ICO',
              sizes=[(n, n) for n in (16, 20, 24, 32, 40, 48, 64, 128, 256)])

with Image.open(root / 'replayer.ico') as icon:
    print('ICO sizes:', sorted(icon.ico.sizes()))
    assert icon.ico.sizes() == {(n, n) for n in (16, 20, 24, 32, 40, 48, 64, 128, 256)}
