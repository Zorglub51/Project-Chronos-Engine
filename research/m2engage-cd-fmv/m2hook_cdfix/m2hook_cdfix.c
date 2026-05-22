/*
 * m2hook_cdfix.c — Targeted binary patch fix for the FMV black-screen bug.
 *
 * Investigation chain (see m2hook_vcedump/REPORT.md sessions 1-8):
 *   PopfulMail (and Vasteel 2) cutscenes black-screen on m2engage because
 *   the CD-ROM emulator's state machine gets stuck at phase=6 (RESULT)
 *   after MSG_IN completes. Subsequent CD command writes from the BIOS
 *   are REJECTED by the I/O write dispatcher because the phase isn't 1
 *   or 2.
 *
 * The reject site, found via Ghidra+objdump analysis:
 *
 *   FUN_0008248c, CD-region $1801 (CD_CMD) write handler at 0x827c0:
 *     0x827cc: cmp r0, #1
 *     0x827ce: beq 0x828ae         ; phase=1 → accept (other path)
 *     0x827d0: cmp r0, #2
 *     0x827d2: bne.w 0x824b6       ; ★ phase != 2 → REJECT (skip to exit)
 *     0x827d6: ldr r1, [r4, #0x38] ; (would have started accepting bytes)
 *
 * After chunk N completes and phase=6 (RESULT) is stuck, every command
 * byte the BIOS writes hits the BNE → rejected. So a new READ command
 * never gets received, FUN_00080b58 never runs to advance state, and
 * the engine stays stuck forever.
 *
 * Fix: NOP-out the BNE so command bytes are accepted regardless of phase.
 *
 * The patch: at VMA 0x827d2 (file offset 0x7a7d2), replace 4 bytes
 *   FROM: 7f f4 70 ae   (bne.w 0x824b6)
 *   TO:   00 bf 00 bf   (nop ; nop)
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/mman.h>
#include <sys/stat.h>

#define LOG_PATH       "/tmp/m2_cdfix.log"

/* Patch site signature — the 4 bytes immediately preceding the bne to
 * disambiguate. The 12-byte window we look for spans:
 *   0x827cc: cmp r0, #1            01 28
 *   0x827ce: beq 0x828ae           6e d0
 *   0x827d0: cmp r0, #2            02 28
 *   0x827d2: bne.w 0x824b6         7f f4 70 ae   ← bytes 6-9 in window
 *   0x827d6: ldr r1, [r4, #0x38]   a1 6b           ← bytes 10-11
 */
static const uint8_t g_signature[12] = {
    0x01, 0x28, 0x6e, 0xd0,   /* cmp r0,#1 ; beq 0x828ae */
    0x02, 0x28,               /* cmp r0,#2 */
    0x7f, 0xf4, 0x70, 0xae,   /* bne.w 0x824b6   ← we patch this */
    0xa1, 0x6b,               /* ldr r1, [r4, #0x38] */
};
#define PATCH_OFFSET_IN_SIG  6      /* bne.w starts at offset 6 in sig */
#define PATCH_BYTES_LEN      4
static const uint8_t g_patch_bytes[4] = { 0x00, 0xbf, 0x00, 0xbf };  /* nop ; nop */

/* ---- find m2engage r-xp mapping ---- */

static int find_code_section(uintptr_t *out_addr, size_t *out_size)
{
    char path[64];
    snprintf(path, sizeof(path), "/proc/%d/maps", (int)getpid());
    *out_addr = 0; *out_size = 0;
    FILE *f = fopen(path, "r"); if (!f) return -1;
    char line[512];
    while (fgets(line, sizeof(line), f)) {
        if (strstr(line, "r-xp") && strstr(line, "m2engage")) {
            unsigned long s, e;
            if (sscanf(line, "%lx-%lx", &s, &e) == 2) {
                *out_addr = s; *out_size = e - s; fclose(f); return 0;
            }
        }
    }
    fclose(f); return -1;
}

static uint8_t *find_pattern(uint8_t *data, size_t size,
                             const uint8_t *pat, size_t patlen)
{
    if (size < patlen) return NULL;
    for (size_t i = 0; i + patlen <= size; i += 2) {
        if (memcmp(data + i, pat, patlen) == 0) return data + i;
    }
    return NULL;
}

__attribute__((constructor))
static void m2hook_cdfix_init(void)
{
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) return;

    FILE *log = fopen(LOG_PATH, "a");
    if (!log) { fprintf(stderr, "[CDFIX] cannot open %s\n", LOG_PATH); return; }
    setvbuf(log, NULL, _IOLBF, 0);

    fprintf(log, "\n[CDFIX] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);

    uintptr_t code_addr = 0; size_t code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(log, "[CDFIX] ERROR: no m2engage r-xp\n");
        fclose(log); return;
    }

    /* Find the signature — should be unique. */
    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) {
        fprintf(log, "[CDFIX] ERROR: signature not found\n");
        fclose(log); return;
    }
    uintptr_t sig_addr = (uintptr_t)match;
    fprintf(log, "[CDFIX] signature found at 0x%08x\n", (unsigned)sig_addr);

    /* Check for duplicate occurrences. */
    uint8_t *dup = find_pattern(match + 2, code_size - (match - (uint8_t*)code_addr) - 2,
                                 g_signature, sizeof(g_signature));
    if (dup) {
        fprintf(log, "[CDFIX] ERROR: signature has duplicate at 0x%08x\n",
                (unsigned)(uintptr_t)dup);
        fclose(log); return;
    }

    uintptr_t patch_addr = sig_addr + PATCH_OFFSET_IN_SIG;
    fprintf(log, "[CDFIX] patch target VMA 0x%08x (NOP-out bne.w)\n",
            (unsigned)patch_addr);

    /* mprotect RW the page(s) containing the patch. */
    uintptr_t page_start = patch_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(log, "[CDFIX] ERROR: mprotect RWX: %s\n", strerror(errno));
        fclose(log); return;
    }

    /* Verify current bytes match expected (defense against wrong binary). */
    uint8_t *p = (uint8_t *)patch_addr;
    static const uint8_t expected[4] = { 0x7f, 0xf4, 0x70, 0xae };
    if (memcmp(p, expected, 4) != 0) {
        fprintf(log, "[CDFIX] ERROR: patch site bytes don't match expected:\n");
        fprintf(log, "       have %02x %02x %02x %02x, expected 7f f4 70 ae\n",
                p[0], p[1], p[2], p[3]);
        mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
        fclose(log); return;
    }

    /* Apply patch: 4-byte BNE.W → 2x NOP T1. */
    memcpy(p, g_patch_bytes, PATCH_BYTES_LEN);

    /* Restore RX. */
    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)p, (char *)p + PATCH_BYTES_LEN);

    fprintf(log, "[CDFIX] PATCHED — 4 bytes at 0x%08x now NOP\n",
            (unsigned)patch_addr);
    fprintf(log, "[CDFIX] effect: CD command-byte writes ($1801) accepted "
                 "regardless of CD-ROM phase. Should unstick PopfulMail FMV.\n");
    fflush(log);
    fclose(log);
}
