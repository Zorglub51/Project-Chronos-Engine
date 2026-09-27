#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 ]]; then
    echo "Usage: $0 <release executable> <output directory>" >&2
    exit 2
fi
if [[ $(id -u) == 0 ]]; then
    echo 'Run packaging as a regular user; makepkg must not run as root.' >&2
    exit 2
fi
recipe=$(cd -- "$(dirname -- "$0")" && pwd)
repo=$(cd -- "$recipe/../../.." && pwd)
binary=$(realpath -- "$1")
mkdir -p -- "$2"
output=$(realpath -- "$2")
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT

cp -- "$recipe/PKGBUILD" "$recipe/com.pce.game-editor.desktop" "$stage/"
install -m755 -- "$binary" "$stage/pce-game-editor"
strip --strip-unneeded "$stage/pce-game-editor"
cp -- "$repo/editor/src-tauri/icons/128x128@2x.png" "$stage/com.pce.game-editor.png"
cp -- "$repo/LICENSE" "$stage/LICENSE.txt"
cp -- "$repo/editor/crates/m2-publish/fonts/OFL.txt" "$stage/NotoSansCJK-OFL.txt"
cp -- "$repo/docs/ARCH-LINUX.md" "$stage/README.md"

python3 - "$repo" "$stage" <<'PY'
import hashlib, pathlib, subprocess, sys, tomllib
repo, stage = map(pathlib.Path, sys.argv[1:])
binary = (stage / 'pce-game-editor').read_bytes()
font = (repo / 'editor/crates/m2-publish/fonts/NotoSansCJKjp-Medium.otf').read_bytes()
assert binary[:4] == b'\x7fELF' and binary[4:6] == b'\x02\x01', 'Expected a 64-bit little-endian ELF'
assert int.from_bytes(binary[18:20], 'little') == 62, 'Expected x86_64'
assert binary.count(font) == 1, 'The complete Japanese font must be embedded exactly once'
lock = tomllib.loads((repo / 'editor/Cargo.lock').read_text())
converter = next(p['source'].rsplit('#', 1)[1] for p in lock['package'] if p['name'] == 'pcd-core')
info = [
    'Platform: Arch Linux x86_64',
    'Editor commit: ' + subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip(),
    'Converter commit: ' + converter,
    'Executable SHA-256: ' + hashlib.sha256(binary).hexdigest(),
    'Complete Japanese font copies: 1',
    subprocess.check_output(['rustc', '--version'], text=True).strip(),
    subprocess.check_output(['pacman', '-Q', 'glibc', 'webkit2gtk-4.1', 'gtk3'], text=True).strip(),
]
(stage / 'BUILD-INFO.txt').write_text('\n'.join(info) + '\n')
PY

desktop-file-validate "$stage/com.pce.game-editor.desktop"
(
    cd -- "$stage"
    makepkg --geninteg >> PKGBUILD
    PKGDEST="$output" makepkg --cleanbuild --noconfirm
)
cp -- "$stage/BUILD-INFO.txt" "$output/BUILD-INFO.txt"
cp -- "$stage/PKGBUILD" "$output/PKGBUILD"
(
    cd -- "$output"
    sha256sum ./*.pkg.tar.zst > SHA256SUMS.txt
)
