# Replayer logo

Generated with the built-in image_gen tool on 2026-09-26.
Original output retained without image processing. Logo asset: replayer-logo.png.

## Windows application assets

`replayer-icon-256.png` and `replayer.ico` are resized/encoded derivatives of the
original logo, with transparency preserved. Regenerate using
`uv run --with pillow==12.1.1 python scripts/generate-icons.py`.
The ICO contains 16, 20, 24, 32, 40, 48, 64, 128 and 256 pixel images.

`build.rs` embeds the ICO in the Windows executable; `src/main.rs` supplies the PNG
to the native window. The installer uses the ICO for Setup/Uninstall and the
executable's icon for application shortcuts and the installed-apps entry.
Generated assets are checked into the repository; CI requires no image-generation
service or Python installation.

## Prompt

Use case: logo-brand. Create one finished minimalist logo symbol for replayer, a desktop music and video player. Square 1024x1024 canvas with a genuinely transparent background. A single compact geometric cobalt-blue mark (#5684F5): a thick smoothly rounded open loop suggesting replay, integrated seamlessly with one right-pointing play triangle through clever negative space. The silhouette should subtly suggest a lowercase r without becoming a literal letter illustration. Flat solid color, crisp vector-like edges, balanced optical weight, very few shapes, recognizable at 24 pixels. Center the mark with generous even clear space, occupying about 70 percent of canvas. Quiet, modern, refined, suitable for a Windows application icon and a dark media player interface. No text, no wordmark, no gradients, no shadows, no 3D, no outlines around a tile, no mockup, no multiple options, no decorative audio equalizer bars. Transparent pixels around the symbol, not a checkerboard drawing.
