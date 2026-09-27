# Packed HuCards and titlebars

The ROM picker accepts `.pce.m` HuCards, including original names such as
`Neutopia_II_J.PCE.m`. Import copies the archive byte for byte, with its original
name, without decompression, conversion or BIOS configuration. An identical
archive already present in the game directory is reused. A different archive
with the same name produces an error: MZS encryption depends on the filename,
so the importer must not silently rename it. Other `.m` formats are rejected.

Publishing copies these archives unchanged. Raw `.pce` ROMs continue to be
packed at publication. A raw and a packed ROM targeting the same output name
cannot silently overwrite one another. The native profile omits the final `.m`
because m2engage appends that suffix itself.

The first titlebar choice, **White bar**, stores the native value `0`, as used
by the original JP Neutopia II (`GAME043`). It survives saving, moving between
folders/lineups, and publication to `title_mode_top.psb.m`. All 13 styles are
available in both JP and US lineups, regardless of the game's platform. Moving
a game or changing its platform preserves the chosen style. The former setting
that forced US banners is removed; old settings files no longer enable it.

The console title preview keeps the previous image visible while rendering and
decoding its replacement. Typing is debounced, obsolete results are ignored, and
the preview and error row retain their height when changing language or text.
