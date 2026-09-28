#!/usr/bin/env bash
# Keep graphics-driver companion libraries matched to the host Mesa installation.
# https://github.com/tauri-apps/tauri/issues/15976
set -euo pipefail
input=$(realpath -- "${1:?Usage: finalize.sh <input AppImage> <output AppImage> <linuxdeploy AppImage plugin>}")
output=$(realpath -m -- "${2:?Missing output AppImage}")
plugin=$(realpath -- "${3:?Missing linuxdeploy AppImage plugin}")
[[ $input != "$output" ]] || { echo 'Input and output must differ.' >&2; exit 1; }
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
cd "$stage"
chmod +x "$input"
"$input" --appimage-extract >/dev/null
# GTK/WebKit stay bundled. These libraries belong to the host graphics stack:
# copying Ubuntu versions beside current Mesa breaks EGL even under X11.
find squashfs-root/usr/lib \( -type f -o -type l \) \( \
    -name 'libwayland-*.so*' -o -name 'libxcb*.so*' -o \
    -name 'libxkbcommon*.so*' -o -name 'libgbm.so*' -o \
    -name 'libdrm*.so*' -o -name 'libEGL.so*' -o \
    -name 'libGL.so*' -o -name 'libGLX.so*' -o -name 'libGLdispatch.so*' \
    \) -print -delete
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 LDAI_OUTPUT="$output" \
    "$plugin" --appdir "$stage/squashfs-root"
chmod +x "$output"
