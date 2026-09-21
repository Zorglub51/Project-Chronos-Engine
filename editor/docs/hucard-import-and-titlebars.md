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

The first titlebar choice, **No titlebar**, stores the native value `0`, as used
by the original JP Neutopia II (`GAME043`). It survives saving, moving between
folders/lineups, and publication to `title_mode_top.psb.m`. The US titlebar
setting preserves this explicit absence; it restricts visible banners to TG16
or TG16-CD without preventing the user from choosing no banner.
