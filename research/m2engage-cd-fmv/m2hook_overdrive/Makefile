# Thin wrapper around build.sh — the actual cross-compile uses the
# m2engage-cross Docker image (Ubuntu 20.04 + arm-linux-gnueabihf-gcc),
# same toolchain as m2engage-mac/build-a33.sh.

TARGET = build/m2hook_overdrive.so
SRC    = m2hook_overdrive.c

all: $(TARGET)

$(TARGET): $(SRC) build.sh
	bash build.sh

clean:
	rm -rf build

.PHONY: all clean
