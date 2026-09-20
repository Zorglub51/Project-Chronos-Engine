#!/bin/bash
# Cross-compile m2hook_print.so for the PCE Mini (Allwinner A33, armhf).
# Reuses the m2engage-cross Docker image built by m2engage-mac/build-a33.sh.

set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGE_NAME="m2engage-cross"

if ! docker image inspect "$IMAGE_NAME" >/dev/null 2>&1; then
    echo "=== Building Docker cross-compilation image ==="
    docker build -t "$IMAGE_NAME" -f "$SCRIPT_DIR/../m2engage-mac/Dockerfile.cross" "$SCRIPT_DIR/../m2engage-mac"
fi

mkdir -p "$SCRIPT_DIR/build"

docker run --rm \
    -v "$SCRIPT_DIR:/src:ro" \
    -v "$SCRIPT_DIR/build:/build" \
    "$IMAGE_NAME" \
    bash -c '
        cd /build
        arm-linux-gnueabihf-gcc \
            -shared -fPIC -Wall -Wextra -O2 -g \
            -mcpu=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard \
            -o m2hook_print.so /src/m2hook_print.c \
            -ldl -lrt -pthread -Wl,--no-as-needed
    '

echo "=== Build complete ==="
file "$SCRIPT_DIR/build/m2hook_print.so" 2>/dev/null || echo "(no file cmd)"
ls -la "$SCRIPT_DIR/build/m2hook_print.so"
