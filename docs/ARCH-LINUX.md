# Arch Linux editor

The Arch Linux build targets **x86_64 desktop PCs**. It includes the library
editor, the CUE/PCD converter, Chronos publishing assets, and the complete Noto
Japanese font. Games, BIOS files, and original console resources are not bundled.

## Install and launch

Download `pce-game-editor-1.0.0alpha-4-x86_64.pkg.tar.zst` from the
[GitHub releases](https://github.com/Zorglub51/Project-Chronos-Engine/releases).
From the download directory, update Arch and install the local package:

```sh
sudo pacman -Syu
sudo pacman -U ./pce-game-editor-1.0.0alpha-4-x86_64.pkg.tar.zst
```

Pacman resolves the declared dependencies from Arch's repositories, including
GTK 3 and WebKitGTK 4.1. Launch **PCE Game Editor** from your application menu,
or run `pce-game-editor` in a terminal. The package is distributed through GitHub;
it is not an AUR or official Arch repository package.

This is a native Arch package, not a statically linked executable. It requires
an up-to-date Arch installation and a graphical desktop. It is not an ARM build
and does not run on the console itself. The editor works on mounted libraries;
mount the USB stick using your desktop first, then select its root directory.
No root privileges are needed to run the editor.

The same library format is used on Windows, macOS, and Linux. Settings use the
standard Linux application configuration/data directories. Library files remain
where you create them. To uninstall the application:

```sh
sudo pacman -R pce-game-editor
```

This does not remove your libraries or user settings.

## Graphics compatibility

Package revision 2 and later select WebKitGTK's non-DMA-BUF renderer by default on Linux.
This works around `Failed to create GBM buffer ... Invalid argument` and blank
windows reported with some graphics drivers. It applies both to terminal and
application-menu launches, without changing your display session or system-wide
graphics configuration. The WebKit sandbox remains enabled in normal use.

For revision 1, the equivalent temporary workaround is:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 pce-game-editor
```

An explicit environment setting is preserved. To opt back into the DMA-BUF
renderer on a compatible driver, launch with
`WEBKIT_DISABLE_DMABUF_RENDERER=0 pce-game-editor`.
See the [upstream WebKit report](https://bugs.webkit.org/show_bug.cgi?id=280210).

## Build and package

Native builds run in the private converter repository, as for Windows and
macOS. Its **Build Chronos editor for Arch Linux** workflow checks out a specified
public editor revision and resolves the converter dependency from its private
checkout. It builds inside the official `archlinux:base-devel` container.

Building locally requires access to the pinned private converter source:

```sh
sudo pacman -Syu --needed base-devel git rust nodejs npm cmake python \
  gtk3 webkit2gtk-4.1 openssl librsvg xdotool hicolor-icon-theme ttf-dejavu \
  desktop-file-utils
cd editor
export CMAKE_POLICY_VERSION_MINIMUM=3.5
export OPUS_NO_PKG=1 OPUS_STATIC=1
npm ci
cargo test --workspace --locked
node --test tests/*.test.cjs
npm run build -- --ci --no-bundle -- --locked
bash packaging/arch/package.sh target/release/pce-game-editor target/arch
```

Run these commands as your regular user. `package.sh` stages the executable,
desktop entry, icon, license notices and build identity, generates checksums for
the local sources, then calls `makepkg`. It checks that only one complete copy
of the Japanese font is embedded. The packaging recipe is not a standalone
source download recipe for the AUR.

The workflow runs converter, publisher/importer and frontend tests, then installs
the package in a separate Arch container without Rust, Node.js or build tools.
It checks dynamic library resolution and renders the installed application's
welcome screen under Xvfb. The screenshot and application log accompany the
CI artifacts. The renderer setting is unset by the test so the installed
application's own default is exercised. This headless X11 check does not
validate every desktop compositor
or a physical USB stick. Any WebKit sandbox override used by the isolated CI
test is confined to that test; none is installed with the application.

References: [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/),
[Arch package format](https://man.archlinux.org/man/PKGBUILD.5).
