# Thin wrapper around build.sh — actual cross-compile uses the
# m2engage-cross Docker image (Ubuntu 20.04 + arm-linux-gnueabihf-gcc).

TARGET = build/m2hook_vramdump.so
SRC    = m2hook_vramdump.c

all: $(TARGET)

$(TARGET): $(SRC) build.sh
	bash build.sh

clean:
	rm -rf build

.PHONY: all clean
