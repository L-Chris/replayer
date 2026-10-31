# Pordo logo

The current logo was generated with the built-in `image_gen` tool on 2026-10-24.
The reference was the small folded play mark at the top-left of the approved
[home design](../../docs/design/replayer-v1/00-home.png).

The user requested a solid center. The final image fills the former central
opening while preserving the rounded silhouette, mint and blue-teal palette,
and folded surfaces. `pordo-logo.png` is the unmodified generated output;
its SHA-256 matches the selected image-generation artifact.

## Application assets

- `pordo-logo.png`: original transparent artwork.
- `pordo-icon-256.png`: window/application icon with original clear space.
- `pordo-mark-256.png`: same artwork with compact padding for the sidebar.
- `pordo.ico`: Windows icons at 16, 20, 24, 32, 40, 48, 64, 128 and 256px.
- `pordo.icns`: macOS application/Dock icon with Retina variants through 1024px,
  resized from the same approved original. The macOS bundle references it in Info.plist.

Regenerate the resized/encoded derivatives from the checkout in PowerShell:

```powershell
$env:UV_CACHE_DIR = "$PWD/target/tool-cache/uv"
$env:UV_PYTHON_INSTALL_DIR = "$PWD/target/tool-cache/python"
uv run --no-project scripts/generate-icons.py
```

The script preserves the original artwork and validates transparent corners,
a solid center and all ICO sizes. Generated assets are checked in; Windows CI
needs no Python or image-generation service.

`src/app/branding.rs` decodes the sidebar artwork once, then downsamples it with
premultiplied-alpha Lanczos filtering to the actual physical display size. The
texture is cached per egui context and replaced only when its pixel size changes
(for example, when moving between monitors with different display scales). Its
rectangle is aligned to physical pixels to avoid a second fractional resampling.
All pages with the main sidebar use this mark. Immersive players and the comic
reader keep their existing layout without a brand header.

`build.rs` embeds the ICO in the executable. `src/main.rs` supplies the PNG to the
native window. Packaging also copies the ICO to `icons/pordo-<sha256-prefix>.ico`.
The installer uses that content-addressed file for Setup, desktop/Start Menu
shortcuts and the installed-apps entry. This gives changed artwork a new Windows
icon-cache key, including same-version rebuilds; existing icon files are retained
for older custom shortcuts until uninstall.

After installation, existing taskbar shortcut copies targeting this installation's
`pordo.exe` have only their icon location updated and saved. Their target,
arguments and application identity are retained. The installer sends targeted
`SHCNE_UPDATEITEM` notifications for the executable and affected shortcuts; it
neither restarts Explorer nor clears other applications' icon caches. The app's
inferred taskbar identity is kept rather than introducing a new AppUserModelID
that would split existing pinned groups.

Windows CI runs `scripts/test-installed-icons.ps1` against the built installer.
It checks desktop, Start Menu and existing taskbar links on installation and
reinstallation, including an unchecked desktop task, and verifies an unrelated
pinned link is unchanged. Manual verification should also check the visible
pinned/running taskbar icon after upgrading an older installation.

## Initial generation

Built-in `image_gen`, transparent background. Reference: `00-home.png`.
Generate a standalone logo matching the small mint/jade and blue-teal folded
play mark at the top-left of the reference UI. Preserve the rounded silhouette,
folded ribbon construction and restrained color palette. Center one mark on a
square transparent canvas. No text, tile, background, shadow or extra symbols.

## Final edit prompt (verbatim)

Use case: precise-object-edit. Edit the attached pordo logo. Make exactly one design change: remove the central triangular hole and fill that entire opening with the logo's blue-teal surface so the mark becomes one continuous SOLID right-pointing play silhouette, with absolutely no hollow center or transparent interior. Blend the filled center naturally into the folded blue-teal upper surface. Keep the mint front ribbon crossing the bottom-right and the restrained fold shading, but do not create a smaller triangle, a new inset outline, a dark cavity, or a second play symbol in the center. Invariants: preserve the outer rounded triangular silhouette, its orientation, overall proportions, position and canvas size, mint/jade and blue-teal palette, folded surface quality, and clean edges. Preserve genuine transparency OUTSIDE the solid silhouette only. No text, no tile, no shadow, no added symbols.
