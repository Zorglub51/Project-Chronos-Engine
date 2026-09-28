#!/usr/bin/env bash
# Exercise the installed application in a disposable Linux desktop session.
set -euo pipefail
output=$(realpath -m -- "${1:?Usage: linux-smoke.sh <output directory>}")
mkdir -p -- "$output"
export XDG_CONFIG_HOME="$output/config"
export XDG_DATA_HOME="$output/data"
export XDG_CACHE_HOME="$output/cache"
export XDG_RUNTIME_DIR="$output/runtime"
mkdir -p -- "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
export GDK_BACKEND=x11
export LIBGL_ALWAYS_SOFTWARE=1
# Exercise the application's own Linux renderer default, not a test-only fix.
unset WEBKIT_DISABLE_DMABUF_RENDERER

binary=${2:-/usr/bin/pce-game-editor}
if [[ $binary != *.AppImage ]]; then
ldd "$binary" > "$output/ldd.txt"
if grep -q 'not found' "$output/ldd.txt"; then
    cat "$output/ldd.txt" >&2
    exit 1
fi
pacman -Qkk pce-game-editor > "$output/package-check.txt"
fi
"$binary" > "$output/application.log" 2>&1 &
app=$!
trap 'kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true' EXIT

for ((attempt=0; attempt<45; attempt++)); do
    if ! kill -0 "$app" 2>/dev/null; then
        cat "$output/application.log" >&2
        echo 'Editor exited during startup.' >&2
        exit 1
    fi
    window=$(xdotool search --onlyvisible --name '^PCE Game Editor$' 2>/dev/null | head -1 || true)
    if [[ -n $window ]]; then
        xdotool mousemove 0 0
        maim -i "$window" "$output/welcome.png"
        # OCR otherwise drops white text on the colored buttons. Keep the
        # original screenshot as evidence and normalize a separate OCR input.
        python3 - "$output/welcome.png" "$output/welcome-ocr.png" <<'PY'
from PIL import Image, ImageOps
import sys
image = ImageOps.grayscale(Image.open(sys.argv[1]))
image = image.point(lambda pixel: 0 if pixel >= 140 else 255)
image.resize((image.width * 3, image.height * 3)).save(sys.argv[2])
PY
        tesseract "$output/welcome-ocr.png" "$output/welcome" --psm 6 2>/dev/null
        if grep -qi 'New library from a console dump' "$output/welcome.txt" &&
           grep -qi 'Open an existing library' "$output/welcome.txt"; then
            break
        fi
    fi
    sleep 1
done
if ! grep -qi 'New library from a console dump' "$output/welcome.txt" ||
   ! grep -qi 'Open an existing library' "$output/welcome.txt"; then
    cat "$output/application.log" "$output/welcome.txt" >&2
    echo 'The installed editor did not render both library actions.' >&2
    exit 1
fi
xdotool getwindowgeometry --shell "$window" > "$output/window.txt"
kill -0 "$app"
echo 'Installed editor rendered the new/open library screen successfully.'
