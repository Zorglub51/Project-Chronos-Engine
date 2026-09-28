# Linux AppImage

The x86_64 AppImage includes the editor, integrated CUE/PCD converter, complete
Japanese font and graphical libraries. No separately installed converter is needed.
Download the AppImage and its SHA-256 checksum from the GitHub release, then run:

```sh
sha256sum -c PCE-Game-Editor-linux-x86_64.AppImage.sha256
chmod +x PCE-Game-Editor-linux-x86_64.AppImage
./PCE-Game-Editor-linux-x86_64.AppImage
```

Use the actual downloaded filename in these commands. A graphical Linux desktop with its standard Fontconfig font libraries
is required; GTK and WebKit are bundled. The build uses Ubuntu 22.04 (glibc 2.35) as its compatibility baseline;
older distributions are not supported. Graphics-driver companion libraries
(Wayland, XCB, XKB, EGL/GL, GBM and DRM) are supplied by the desktop system,
so they match its Mesa or proprietary driver. The packaging step removes older
copies of these libraries that Tauri would otherwise bundle. The editor preserves its Linux graphics
compatibility default, including the workaround needed on some Arch systems.

If FUSE is unavailable, run without mounting the AppImage:

```sh
APPIMAGE_EXTRACT_AND_RUN=1 ./PCE-Game-Editor-linux-x86_64.AppImage
```

Libraries and settings are stored outside the AppImage, so replacing the application
file does not replace your game library. Console resources and BIOS files must still
be imported from your own dump.

Builds run in the private converter repository using `editor-appimage.yml`, with
an explicit public editor revision. CI tests the Rust workspace and frontend, then
checks the packaged welcome screen on Ubuntu 22.04 and Arch Linux. The headless
container tests use extract-and-run and disable the WebKit sandbox only inside
those disposable test sessions. These overrides are not built into the package.
