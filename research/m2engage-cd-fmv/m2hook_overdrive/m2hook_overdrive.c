/*
 * m2hook_overdrive.c — Observation hook for m2engage's "opcode 44" dispatcher.
 *
 * Patches the 8-byte prologue of the dispatcher at VMA 0x34470 (file offset
 * 0x2C470 in the stock JP m2engage, MD5 060f4815731c0d0717ee018665ab4a2c) with
 * a redirect to a Thumb trampoline that calls a C handler before replaying
 * the displaced instructions and resuming the original code.
 *
 * The C handler logs every dispatcher call (opcode histogram) and, on
 * opcode 44 (setOverdrive), dumps the backend object header, the resolved
 * leaf handler pointer (*(backend + 0x24)), the value, and 4 KB of code
 * starting at the leaf — enough for an offline disassembler to recover
 * the case-44 path and answer "what does overdrive do".
 *
 * Pure observer: r0..r3 are preserved across the trampoline; no emulation
 * behaviour is altered.
 *
 * Cross-compile (Mini target, A33 / Cortex-A7 armhf):
 *   bash build.sh       # uses the m2engage-cross Docker image
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <time.h>
#include <sys/mman.h>
#include <sys/stat.h>

#define LOG_PATH       "/tmp/m2_overdrive_hook.log"
#define OP_OVERDRIVE   44
#define CODE_DUMP_SIZE 4096

/* The 16-byte signature from SPEC §3 — verified unique in the stock m2engage. */
static const uint8_t g_signature[16] = {
    0x0c, 0xb4, 0x00, 0xb5, 0x83, 0xb0, 0x43, 0x6a,
    0x23, 0xb1, 0x05, 0xaa, 0x08, 0x46, 0x04, 0x99,
};
/* First 8 bytes — the displaced instructions we'll overwrite + replay. */
static const uint8_t g_expected_prologue[8] = {
    0x0c, 0xb4, 0x00, 0xb5, 0x83, 0xb0, 0x43, 0x6a,
};

/* Shared with the asm trampoline below. */
uint32_t g_resume_addr;          /* (dispatcher_addr + 8) | 1  (Thumb bit set) */

/* State. */
static FILE    *g_log;
static volatile unsigned g_op_counts[256];
static volatile int      g_od_hits;
static int      g_armed;

/* ---- C handler — called from the trampoline with r0..r3 intact ---- */

void overdrive_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    /* Histogram of every dispatcher call. */
    if (r2 < 256) g_op_counts[r2]++;
    if (r2 != OP_OVERDRIVE) return;
    if (!g_log) return;

    g_od_hits++;
    uint32_t backend = r0;
    uint32_t handler = r1;
    uint32_t value   = r3;

    /* The unknown leaf function pointer. */
    uint32_t H     = *(volatile uint32_t *)(uintptr_t)(backend + 0x24);
    uint32_t Hcode = H & ~1u;        /* strip Thumb bit for raw addressing */

    fprintf(g_log,
        "[OVRD] hit #%d  opcode=44 (setOverdrive)\n"
        "       backend = 0x%08x\n"
        "       handler = 0x%08x\n"
        "       value   = %d (0x%08x)\n"
        "       *(backend+0x24) = 0x%08x  -> handler code @ 0x%08x\n",
        g_od_hits, backend, handler, (int)value, value, H, Hcode);

    /* Backend object header — 0x00..0x30 inclusive. */
    fprintf(g_log, "       backend[0x00..0x30]:");
    for (int i = 0; i <= 0x30; i += 4) {
        uint32_t w = *(volatile uint32_t *)(uintptr_t)(backend + i);
        fprintf(g_log, " %08x", w);
    }
    fprintf(g_log, "\n");

    /* Code dump of the leaf handler — for offline disassembly. */
    fprintf(g_log, "       --- code dump @ 0x%08x (%d bytes) ---\n",
            Hcode, CODE_DUMP_SIZE);
    for (int i = 0; i < CODE_DUMP_SIZE; i += 16) {
        const uint8_t *p = (const uint8_t *)(uintptr_t)(Hcode + i);
        fprintf(g_log, "       %08x:", Hcode + i);
        for (int j = 0; j < 16; j++) {
            fprintf(g_log, " %02x", p[j]);
        }
        fprintf(g_log, "\n");
    }
    fprintf(g_log, "       --- end dump ---\n");
    fflush(g_log);
}

/* ---- Trampoline (Thumb naked) ----
 *
 * Layout (matches SPEC §4):
 *   1. push {r0-r3, r12, lr}        ; 24 bytes — preserves dispatcher args + AAPCS scratch
 *   2. bl   overdrive_handler        ; receives (r0,r1,r2,r3) intact
 *   3. pop  {r0-r3, r12, lr}         ; restore exactly
 *   4. replay the 4 displaced instructions:
 *        push {r2, r3}
 *        push {lr}
 *        sub  sp, #12
 *        ldr  r3, [r0, #36]
 *   5. ldr r12, =g_resume_addr ; ldr r12,[r12] ; bx r12  (jump to dispatcher_addr+8 | 1)
 */
__attribute__((naked, used, target("thumb")))
void overdrive_trampoline(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        /* Save dispatcher args and AAPCS caller-saved scratch.
         * push {r0-r3, r12, lr} = 6 regs * 4 bytes = 24 bytes (8-byte aligned). */
        "push {r0, r1, r2, r3, r12, lr}\n"

        /* Call C handler — args (r0,r1,r2,r3) already in place. */
        "bl   overdrive_handler\n"

        /* Restore exactly. */
        "pop  {r0, r1, r2, r3, r12, lr}\n"

        /* Replay displaced instructions from 0x34470..0x34477. */
        "push {r2, r3}\n"
        "push {lr}\n"
        "sub  sp, #12\n"
        "ldr  r3, [r0, #36]\n"

        /* Resume at dispatcher_addr + 8 with Thumb bit set.
         * r12 is caller-saved and not used by the dispatcher's code at +8,
         * so clobbering it for the indirect jump is safe. */
        "ldr  r12, =g_resume_addr\n"
        "ldr  r12, [r12]\n"
        "bx   r12\n"

        ".ltorg\n"
    );
}

/* ---- Pattern scan within the m2engage r-x mapping ---- */

static int find_code_section(uintptr_t *out_addr, size_t *out_size)
{
    char path[64];
    snprintf(path, sizeof(path), "/proc/%d/maps", (int)getpid());

    *out_addr = 0;
    *out_size = 0;

    FILE *f = fopen(path, "r");
    if (!f) return -1;

    char line[512];
    while (fgets(line, sizeof(line), f)) {
        if (strstr(line, "r-xp") && strstr(line, "m2engage")) {
            unsigned long s, e;
            if (sscanf(line, "%lx-%lx", &s, &e) == 2) {
                *out_addr = (uintptr_t)s;
                *out_size = (size_t)(e - s);
                fclose(f);
                return 0;
            }
        }
    }
    fclose(f);
    return -1;
}

static uint8_t *find_pattern(uint8_t *data, size_t size,
                             const uint8_t *pat, size_t patlen)
{
    if (size < patlen) return NULL;
    /* Thumb code is 2-byte aligned. */
    for (size_t i = 0; i + patlen <= size; i += 2) {
        if (memcmp(data + i, pat, patlen) == 0) return data + i;
    }
    return NULL;
}

/* ---- atexit: dump opcode histogram ---- */

static void dump_histogram(void)
{
    if (!g_log) return;
    fprintf(g_log, "[OVRD] opcode census (dispatcher calls by opcode):\n");
    int any = 0;
    for (int i = 0; i < 256; i++) {
        if (g_op_counts[i]) {
            fprintf(g_log, "       opcode %3d: %u\n", i, g_op_counts[i]);
            any = 1;
        }
    }
    if (!any) fprintf(g_log, "       (no dispatcher calls observed)\n");
    fprintf(g_log, "[OVRD] total opcode-44 hits: %d\n", g_od_hits);
    fflush(g_log);
}

/* ---- Constructor ---- */

__attribute__((constructor))
static void m2hook_overdrive_init(void)
{
    /* Only attach inside m2engage. */
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) {
        return;
    }

    /* Open log file (append). */
    g_log = fopen(LOG_PATH, "a");
    if (!g_log) {
        /* Best-effort: write to stderr if log fails so the user sees something. */
        fprintf(stderr, "[OVRD] cannot open %s: %s\n", LOG_PATH, strerror(errno));
        return;
    }
    setvbuf(g_log, NULL, _IOLBF, 0);

    time_t now = time(NULL);
    fprintf(g_log, "\n[OVRD] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);
    fprintf(g_log, "[OVRD] time: %s", ctime(&now));

    /* Locate m2engage code mapping. */
    uintptr_t code_addr = 0;
    size_t    code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(g_log, "[OVRD] ERROR: could not find m2engage r-xp mapping\n");
        return;
    }
    fprintf(g_log, "[OVRD] code section: 0x%08x .. 0x%08x (%zu bytes)\n",
            (unsigned)code_addr, (unsigned)(code_addr + code_size), code_size);

    /* Find the dispatcher by signature scan. */
    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) {
        fprintf(g_log, "[OVRD] ERROR: dispatcher signature not found\n");
        return;
    }
    uintptr_t disp_addr = (uintptr_t)match;
    fprintf(g_log, "[OVRD] dispatcher found at 0x%08x\n", (unsigned)disp_addr);

    /* Verify the first 8 bytes are exactly the expected displaced prologue. */
    if (memcmp(match, g_expected_prologue, 8) != 0) {
        fprintf(g_log, "[OVRD] ERROR: prologue mismatch — wrong binary?\n");
        return;
    }

    /* resume_addr = dispatcher_addr + 8 with Thumb bit set. */
    g_resume_addr = (uint32_t)(disp_addr + 8) | 1u;

    /* mprotect page(s) → RWX. The 8-byte patch may straddle a page boundary
     * — request 0x2000 from the page start to cover that case. */
    uintptr_t page_start = disp_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(g_log, "[OVRD] ERROR: mprotect RWX failed: %s\n", strerror(errno));
        return;
    }

    /* Write redirect:
     *   F8DF F000  LDR.W PC, [PC, #0]
     *   <4-byte trampoline VMA>
     *
     * GCC emits an ARM veneer for a Thumb naked function; the veneer's
     * address (no Thumb bit) is what we want — its first instruction will
     * switch back to Thumb if needed. Same convention as m2hook_print.c. */
    uint16_t *thumb = (uint16_t *)match;
    uint32_t  hook_addr = (uint32_t)(uintptr_t)overdrive_trampoline;

    thumb[0] = 0xF8DF;
    thumb[1] = 0xF000;
    thumb[2] = (uint16_t)(hook_addr & 0xFFFF);
    thumb[3] = (uint16_t)((hook_addr >> 16) & 0xFFFF);

    /* Restore RX. */
    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)match, (char *)match + 8);

    g_armed = 1;
    fprintf(g_log, "[OVRD] hook armed at 0x%08x -> trampoline 0x%08x "
                   "(resume 0x%08x)\n",
            (unsigned)disp_addr, hook_addr, (unsigned)g_resume_addr);
    fflush(g_log);

    atexit(dump_histogram);
}
