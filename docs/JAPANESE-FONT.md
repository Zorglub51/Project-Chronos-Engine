# Japanese fonts and console title preview

The editor publishes Noto Sans CJK JP Medium at the three native menu sizes:
18, 24 and 32 pixels. Every existing text glyph is replaced, and characters used
by either lineup, including folder names, are added automatically. M2 controller
and interface pictograms keep their original artwork. English-specific font
resources are unchanged; the native title renderer uses the Japanese 32-pixel
resource for both Japanese and translated game titles.

The complete font remains on the PC. The A33 receives padded A8 atlas pages,
at most 1024 × 1024 each, containing the stock menu character set plus the
library's characters. There is no new console rasterizer, Squirrel code or
per-frame font processing. Missing Noto characters cause a visible publication
error instead of silently exporting missing-glyph boxes.

The original M2 engine also drops characters outside the Unicode Basic
Multilingual Plane (above U+FFFF), even when Noto and the PSB contain the glyph.
The VM reproduced this with `𠮷` (U+20BB7). Preview and publication explicitly
reject such a character and request an alternative spelling, such as `吉`.
This is an engine limitation, not missing font coverage. BMP variants such as
`髙` and `﨑`, and the added kanji `魍魎`, render correctly.

## Publication and existing libraries

Publishing writes the three `makoto_basefont*.psb.m` Japanese resources to
`library/published/system/font/`. In the canonical USB layout it also installs
them into `game/system/font/`, together with the font license. Restart the
console menu after replacing fonts because M2 retains loaded font resources.
Standalone exports must copy `system/font/` into their runtime's `game/` tree.
The VM preparation tool automatically overlays these published fonts.

New libraries retain the original font PSBs and `titleselect_ui.psb.m` in
`library/templates/`. Older libraries can use their sibling `game/` or
`BACKUP/game/`; publication preserves original font templates before installing
the replacement. If these assets are missing, the editor names the missing
resource. Restore it from the original console dump under `library/templates/`.
Font files are generated before ROMs or packs are written, so a missing glyph
does not partially publish a catalogue.

## Preview

Each game's edit screen shows the full 1280-pixel title strip, using the
original titlebar motion's sprites, origins, colors and transforms. Choose the
menu language with the Japanese / English buttons, independently of the game's
lineup. The selected text follows
the same name fallback rules as publication. Style zero retains the native
plain strip, without a platform badge. The titlebar dropdown shows the original
badge graphics and a white strip for style zero;
the preview always fits the full strip to the panel's available width.
The game editor heading shows the English name followed by the Japanese name,
separated by a slash; an empty name is omitted.

Text uses the same 32-pixel glyph bitmaps as publication, centered as M2 centers
them, with the native 700-pixel width limit. Longer titles are compressed
horizontally using nearest atlas sampling. The renderer reproduces the VM's
GLES 8-bit subpixel coordinate rounding. Frame-offset preferences shift the
whole strip on the screen and do not affect this cropped preview.

Validated on 2026-09-22 with the original ARM32 engine under QEMU in the Ubuntu
VM. Pixel comparisons across the opaque 1280 × 42 band found no differences
for a Latin title, mixed Japanese/Latin with added `魍魎`, a 1201-pixel title
compressed to 700 pixels on a US banner, and the plain style used by Neutopia II.
The NAMCOT banner also matches all 53,760 pixels in this band. Its chevron uses
M2's default doubled RGB color weight (`0x7f7f7fff`), unlike the explicit `bm=0`
modulation used by other colored shapes. Treating it as ordinary modulation
incorrectly darkens the chevron.
Regression fixtures keep only the open-font text and flat background, excluding
M2 artwork. A33 hardware was not connected for this validation; GPU rounding
can differ between implementations. No menu scripts or real console saves were
modified.

For reproducible native validation, the `font_preview` Cargo example generates
fonts and a reference image from the user's original extracted resources. Its
optional final argument creates an isolated test catalogue in the output
directory. It never modifies the input catalogue or its saves.
