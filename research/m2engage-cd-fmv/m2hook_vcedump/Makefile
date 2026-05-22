TARGET = build/m2hook_vcedump.so
SRC    = m2hook_vcedump.c

all: $(TARGET)

$(TARGET): $(SRC) build.sh
	bash build.sh

clean:
	rm -rf build

.PHONY: all clean
