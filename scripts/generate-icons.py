# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow==12.1.1"]
# ///
"""Rebuild branding assets: uv run scripts/generate-icons.py.

Only resize/encode the generated artwork; preserve its design and transparency.
Generated assets are committed, so CI needs no Python or image generation.
"""
from pathlib import Path
from PIL import Image

root = Path(__file__).resolve().parents[1] / 'assets' / 'branding'
with Image.open(root / 'pordo-logo.png') as source:
    logo = source.convert('RGBA')
print('Source:', logo.size, 'center alpha:', logo.getpixel((logo.width // 2, logo.height // 2))[3])
assert logo.getpixel((logo.width // 2, logo.height // 2))[3] >= 250
sizes = (16, 20, 24, 32, 40, 48, 64, 128, 256)
images = {size: logo.resize((size, size), Image.Resampling.LANCZOS) for size in sizes}
images[256].save(root / 'pordo-icon-256.png')
images[256].save(root / 'pordo.ico', format='ICO',
                 sizes=[(size, size) for size in sizes],
                 append_images=[images[size] for size in sizes if size != 256])
mac_images = [logo.resize((size, size), Image.Resampling.LANCZOS)
              for size in (32, 64, 128, 256, 512, 1024)]
mac_images[-1].save(root / 'pordo.icns', format='ICNS', append_images=mac_images)

# Ignore almost-transparent generation noise when measuring the sidebar crop;
# only the crop bounds change, not the source pixels or their alpha values.
bounds = logo.getchannel('A').point(lambda value: 255 if value >= 32 else 0).getbbox()
mark = logo.crop(bounds)
mark.thumbnail((240, 240), Image.Resampling.LANCZOS)
sidebar = Image.new('RGBA', (256, 256))
sidebar.alpha_composite(mark, ((256 - mark.width) // 2, (256 - mark.height) // 2))
sidebar.save(root / 'pordo-mark-256.png')

with Image.open(root / 'pordo.ico') as icon:
    print('ICO sizes:', sorted(icon.ico.sizes()))
    assert icon.ico.sizes() == {(n, n) for n in (16, 20, 24, 32, 40, 48, 64, 128, 256)}
    for size in sizes:
        image = icon.ico.getimage((size, size)).convert('RGBA')
        assert image.getpixel((0, 0))[3] == 0
        assert image.getpixel((size // 2, size // 2))[3] >= 250
        assert image.getchannel('A').getextrema() == (0, 255)
