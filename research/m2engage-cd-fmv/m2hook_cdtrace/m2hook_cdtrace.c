/*
 * m2hook_cdtrace.c — Observation hook for m2engage's CD-ROM phase logger.
 *
 * Patches the 8-byte prologue of the debug-log function at VMA 0x1eee48
 * (file offset 0x1e6e48 in the stock JP m2engage, MD5
 * 060f4815731c0d0717ee018665ab4a2c) so that we get to inspect every
 * varargs log call before the release-build sink discards it.
 *
 * We capture:
 *   (a) tg16_cdrom_* phase-name strings (r2 points into the rodata block
 *       at 0x1fc4b8..0x1fc618), with consecutive-identical coalescing so
 *       a phase that fires every tick doesn't flood the log.
 *   (b) the six CD-related printf format strings (their VMAs are exact-
 *       matched against r1), rendering up to two int args from r2/r3.
 *
 * Pure observer: r0..r3 are preserved across the trampoline; no behaviour
 * is altered.
 *
 * Cross-compile (Mini target, A33 / Cortex-A7 armhf):
 *   bash build.sh         # uses the m2engage-cross Docker image
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

#define LOG_PATH       "/tmp/m2_cdtrace.log"

/* The 16-byte signature from SPEC §3 — verified unique in the stock m2engage. */
static const uint8_t g_signature[16] = {
    0x0e, 0xb4, 0x00, 0xb5, 0x82, 0xb0, 0x03, 0xaa,
    0x52, 0xf8, 0x04, 0x1b, 0x01, 0x92, 0x18, 0xb1,
};
/* First 8 bytes — displaced instructions to replay. */
static const uint8_t g_expected_prologue[8] = {
    0x0e, 0xb4, 0x00, 0xb5, 0x82, 0xb0, 0x03, 0xaa,
};

/* Shared with the asm trampoline. */
uint32_t g_resume_addr;          /* (logger_addr + 8) | 1  (Thumb bit set) */

/* ---- Rodata bounds (SPEC §5) ---- */
#define RODATA_LO  0x001f6eb8u
#define RODATA_HI  0x003dcb0cu
#define CDPHASE_LO 0x001fc4b8u
#define CDPHASE_HI 0x001fc618u

/* CD-related printf format-string VMAs. */
static const uint32_t g_cd_fmts[] = {
    0x003dc208,  /* "start reading %d"   */
    0x003dc21c,  /* "seek start: %d -> %d" */
    0x003dc234,  /* "play track done"    */
    0x003dc244,  /* "seek %d"            */
    0x003dc24c,  /* "seek done"          */
    0x001fc894,  /* "seek is finished"   */
};
#define G_CD_FMTS_N (sizeof g_cd_fmts / sizeof g_cd_fmts[0])

/* ---- State ---- */
static FILE     *g_log;
static unsigned  g_seq;
static char      g_last_phase[40];
static unsigned  g_phase_repeat;
static int       g_armed;

static void flush_phase(void)
{
    if (g_phase_repeat > 1 && g_log) {
        /* The first occurrence already wrote "ENTER phase:" — only emit a
         * tail count when the phase repeated. */
        fprintf(g_log, "[CD] phase: %-26s x%u\n",
                g_last_phase, g_phase_repeat - 1);
        fflush(g_log);
    }
    g_phase_repeat = 0;
}

/* ---- C handler — called from the trampoline with r0..r3 intact ---- */

void cdtrace_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    (void)r0;
    if (!g_log) return;

    /* (a) phase-name log: r2 points at a tg16_cdrom_* string in rodata. */
    if (r2 >= CDPHASE_LO && r2 < CDPHASE_HI) {
        const char *name = (const char *)(uintptr_t)r2;
        if (strncmp(name, g_last_phase, sizeof g_last_phase) == 0) {
            g_phase_repeat++;
        } else {
            flush_phase();
            strncpy(g_last_phase, name, sizeof g_last_phase - 1);
            g_last_phase[sizeof g_last_phase - 1] = 0;
            g_phase_repeat = 1;
            fprintf(g_log, "[CD] #%u ENTER phase: %s\n", ++g_seq, name);
            fflush(g_log);
        }
        return;
    }

    /* (b) CD format-string log lines — exact VMA match on r1. */
    for (unsigned i = 0; i < G_CD_FMTS_N; i++) {
        if (r1 == g_cd_fmts[i]) {
            flush_phase();
            fprintf(g_log, "[CD] #%u ", ++g_seq);
            /* r1 is a printf format with up to two int specifiers. */
            fprintf(g_log, (const char *)(uintptr_t)r1, (int)r2, (int)r3);
            fputc('\n', g_log);
            fflush(g_log);
            return;
        }
    }
    /* Anything else (non-CD log call) is ignored. */
}

/* ---- Trampoline (Thumb naked) ----
 *
 * Layout:
 *   1. push    {r0-r3, r12, lr}         ; 24 bytes — preserve integer args
 *   2. vpush   {d0-d7}                   ; 64 bytes — preserve FP caller-saved
 *                                        ;   (armhf hardfp: d0-d7 are caller-
 *                                        ;   saved; cdtrace_handler may clobber
 *                                        ;   them via fprintf/strncmp ⇒ the
 *                                        ;   logger's caller would otherwise be
 *                                        ;   silently corrupted at log-rate)
 *   3. bl      cdtrace_handler           ; receives (r0,r1,r2,r3) intact
 *   4. vpop    {d0-d7}
 *   5. pop     {r0-r3, r12, lr}
 *   6. replay the 4 displaced instructions:
 *        push {r1, r2, r3}            ; b40e
 *        push {lr}                    ; b500
 *        sub  sp, #8                  ; b082
 *        add  r2, sp, #12             ; aa03
 *   7. ldr r12, =g_resume_addr ; ldr r12,[r12] ; bx r12
 *
 * Stack delta across (1)+(2) = 24 + 64 = 88 bytes — preserves 8-byte alignment.
 */
__attribute__((naked, used, target("thumb")))
void cdtrace_trampoline(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        /* Save logger args + AAPCS caller-saved scratch. */
        "push {r0, r1, r2, r3, r12, lr}\n"

        /* Save FP caller-saved registers (d0-d7 = q0-q3) — armhf ABI. */
        "vpush {d0-d7}\n"

        /* Call C handler — args (r0,r1,r2,r3) already in place. */
        "bl   cdtrace_handler\n"

        /* Restore. */
        "vpop {d0-d7}\n"
        "pop  {r0, r1, r2, r3, r12, lr}\n"

        /* Replay displaced instructions from 0x1eee48..0x1eee4f. */
        "push {r1, r2, r3}\n"
        "push {lr}\n"
        "sub  sp, #8\n"
        "add  r2, sp, #12\n"

        /* Resume at logger_addr + 8 with Thumb bit set. */
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
    for (size_t i = 0; i + patlen <= size; i += 2) {
        if (memcmp(data + i, pat, patlen) == 0) return data + i;
    }
    return NULL;
}

/* ---- atexit: flush trailing phase ---- */

static void atexit_flush(void)
{
    if (!g_log) return;
    flush_phase();
    fprintf(g_log, "[CD] === end of trace (seq=%u) ===\n", g_seq);
    fflush(g_log);
}

/* ---- Constructor ---- */

__attribute__((constructor))
static void m2hook_cdtrace_init(void)
{
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) {
        return;
    }

    g_log = fopen(LOG_PATH, "a");
    if (!g_log) {
        fprintf(stderr, "[CD] cannot open %s: %s\n", LOG_PATH, strerror(errno));
        return;
    }
    setvbuf(g_log, NULL, _IOLBF, 0);

    time_t now = time(NULL);
    fprintf(g_log, "\n[CD] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);
    fprintf(g_log, "[CD] time: %s", ctime(&now));

    uintptr_t code_addr = 0;
    size_t    code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(g_log, "[CD] ERROR: could not find m2engage r-xp mapping\n");
        return;
    }
    fprintf(g_log, "[CD] code section: 0x%08x .. 0x%08x (%zu bytes)\n",
            (unsigned)code_addr, (unsigned)(code_addr + code_size), code_size);

    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) {
        fprintf(g_log, "[CD] ERROR: logger signature not found\n");
        return;
    }
    uintptr_t log_addr = (uintptr_t)match;
    fprintf(g_log, "[CD] logger found at 0x%08x\n", (unsigned)log_addr);

    if (memcmp(match, g_expected_prologue, 8) != 0) {
        fprintf(g_log, "[CD] ERROR: prologue mismatch — wrong binary?\n");
        return;
    }

    g_resume_addr = (uint32_t)(log_addr + 8) | 1u;

    uintptr_t page_start = log_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(g_log, "[CD] ERROR: mprotect RWX failed: %s\n", strerror(errno));
        return;
    }

    uint16_t *thumb = (uint16_t *)match;
    uint32_t  hook_addr = (uint32_t)(uintptr_t)cdtrace_trampoline;

    thumb[0] = 0xF8DF;
    thumb[1] = 0xF000;
    thumb[2] = (uint16_t)(hook_addr & 0xFFFF);
    thumb[3] = (uint16_t)((hook_addr >> 16) & 0xFFFF);

    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)match, (char *)match + 8);

    g_armed = 1;
    fprintf(g_log, "[CD] hook armed at 0x%08x -> trampoline 0x%08x "
                   "(resume 0x%08x)\n",
            (unsigned)log_addr, hook_addr, (unsigned)g_resume_addr);
    fflush(g_log);

    atexit(atexit_flush);
}
