#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$SCRIPT_DIR/build"
docker run --rm \
    -v "$SCRIPT_DIR:/src:ro" \
    -v "$SCRIPT_DIR/build:/build" \
    m2engage-cross \
    bash -c '
        cd /build
        arm-linux-gnueabihf-gcc \
            -shared -fPIC -Wall -Wextra -O2 -g \
            -mcpu=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard \
            -o m2hook_cdpoke.so /src/m2hook_cdpoke.c \
            -lpthread -ldl -Wl,--no-as-needed
    '
ls -la "$SCRIPT_DIR/build/m2hook_cdpoke.so"
