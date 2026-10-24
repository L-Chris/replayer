# Replayer logo

The current logo was generated with the built-in `image_gen` tool on 2026-10-24.
The reference was the small folded play mark at the top-left of the approved
[home design](../../docs/design/replayer-v1/00-home.png).

The user requested a solid center. The final image fills the former central
opening while preserving the rounded silhouette, mint and blue-teal palette,
and folded surfaces. `replayer-logo.png` is the unmodified generated output;
its SHA-256 matches the selected image-generation artifact.

## Application assets

- `replayer-logo.png`: original transparent artwork.
- `replayer-icon-256.png`: window/application icon with original clear space.
- `replayer-mark-256.png`: same artwork with compact padding for the sidebar.
- `replayer.ico`: Windows icons at 16, 20, 24, 32, 40, 48, 64, 128 and 256px.

Regenerate the resized/encoded derivatives from the checkout in PowerShell:

```powershell
$env:UV_CACHE_DIR = "$PWD/target/tool-cache/uv"
$env:UV_PYTHON_INSTALL_DIR = "$PWD/target/tool-cache/python"
uv run --no-project scripts/generate-icons.py
```

The script preserves the original artwork and validates transparent corners,
a solid center and all ICO sizes. Generated assets are checked in; Windows CI
needs no Python or image-generation service.

`src/app/branding.rs` caches the sidebar artwork once per egui context. All
pages with the main sidebar use this mark. Immersive players and the comic
reader keep their existing layout without a brand header.

`build.rs` embeds the ICO in the executable. `src/main.rs` supplies the PNG to the
native window. The installer uses the ICO for Setup/Uninstall and the
executable's icon for shortcuts and the installed-apps entry.

## Initial generation

Built-in `image_gen`, transparent background. Reference: `00-home.png`.
Generate a standalone logo matching the small mint/jade and blue-teal folded
play mark at the top-left of the reference UI. Preserve the rounded silhouette,
folded ribbon construction and restrained color palette. Center one mark on a
square transparent canvas. No text, tile, background, shadow or extra symbols.

## Final edit prompt (verbatim)

Use case: precise-object-edit. Edit the attached replayer logo. Make exactly one design change: remove the central triangular hole and fill that entire opening with the logo's blue-teal surface so the mark becomes one continuous SOLID right-pointing play silhouette, with absolutely no hollow center or transparent interior. Blend the filled center naturally into the folded blue-teal upper surface. Keep the mint front ribbon crossing the bottom-right and the restrained fold shading, but do not create a smaller triangle, a new inset outline, a dark cavity, or a second play symbol in the center. Invariants: preserve the outer rounded triangular silhouette, its orientation, overall proportions, position and canvas size, mint/jade and blue-teal palette, folded surface quality, and clean edges. Preserve genuine transparency OUTSIDE the solid silhouette only. No text, no tile, no shadow, no added symbols.
