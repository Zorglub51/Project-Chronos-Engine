/* Run under qemu-arm-static with the test libMali preloaded; no GPU needed. */
#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(void) {
    int device = open("/dev/i2c-1", O_RDWR);
    assert(device >= 0);
    char buf[4];
    assert(read(device, buf, sizeof(buf)) == sizeof(buf));
    assert(memcmp(buf, "XGHT", 4) == 0);
    assert(close(device) == 0);

    char path[] = "/tmp/chronos-fd-test-XXXXXX";
    int file = mkstemp(path);
    assert(file >= 0);
    unlink(path);
    assert(file == device);  /* Exercise the actual descriptor reuse. */
    assert(write(file, "ROM!", 4) == 4);
    assert(lseek(file, 0, SEEK_SET) == 0);
    assert(read(file, buf, sizeof(buf)) == sizeof(buf));
    assert(memcmp(buf, "ROM!", 4) == 0);
    assert(close(file) == 0);
    puts("ARM32 fake-device descriptor reuse: OK");
    return 0;
}
