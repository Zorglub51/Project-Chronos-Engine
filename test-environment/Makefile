# Run inside Linux; output may live on the VM's local filesystem.
BUILD ?= build
CC = gcc
ARMCC ?= arm-linux-gnueabihf-gcc
CFLAGS ?= -O2 -Wall -Wextra

.PHONY: all check
all: $(BUILD)/gl_proxy $(BUILD)/libMali.so $(BUILD)/m2hook_print.so

$(BUILD):
	mkdir -p "$@"

$(BUILD)/gl_proxy: graphics/gl_proxy.c graphics/gl_protocol.h graphics/frame_pacing.h | $(BUILD)
	$(CC) $(CFLAGS) -o "$@" $< -lEGL -lGLESv2 -lX11 -lgbm -lpthread -lrt

$(BUILD)/libMali.so: graphics/libMali_x11.c graphics/gl_protocol.h | $(BUILD)
	$(ARMCC) $(CFLAGS) -shared -fPIC -o "$@" $< -ldl -lpthread -lrt

$(BUILD)/m2hook_print.so: ../console-mod/hook-src/m2hook_print.c ../console-mod/hook-src/save_digest.h ../console-mod/hook-src/md5.h ../console-mod/hook-src/save_slice.h ../console-mod/hook-src/folder_worker.h | $(BUILD)
	$(ARMCC) $(CFLAGS) -shared -fPIC -mcpu=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -o "$@" $< -ldl -lrt -pthread

$(BUILD)/test_fd_reuse: test_fd_reuse.c | $(BUILD)
	$(ARMCC) -O2 -Wall -Wextra -o "$@" $<

$(BUILD)/test_frame_pacing: test_frame_pacing.c graphics/frame_pacing.h | $(BUILD)
	$(CC) $(CFLAGS) -o "$@" $<

check: $(BUILD)/libMali.so $(BUILD)/test_fd_reuse $(BUILD)/test_frame_pacing
	PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -v test_native test_save_digest test_save_slice
	"$(BUILD)/test_frame_pacing"
	qemu-arm-static -L /usr/arm-linux-gnueabihf -E LD_PRELOAD=$(abspath $(BUILD))/libMali.so "$(BUILD)/test_fd_reuse"
