# Japanese font

`NotoSansCJKjp-Medium.otf` is the Japanese Medium face from
[Noto CJK](https://github.com/notofonts/noto-cjk/tree/main/Sans/OTF/Japanese),
distributed under the SIL Open Font License 1.1 in `OFL.txt`.

SHA-256: `dd523e580e3413c480b2d701bf64e534c20f8419e3cfb6a44c2bdcd8d2a6c052`.

The full font is embedded once in the desktop publisher. The editor receives
the same bytes through Tauri for Japanese UI text. Title previews and console
resources share the Rust bitmap rasterizer, independent of system fonts.
Only required glyphs are exported; the console never reads this OpenType file.
