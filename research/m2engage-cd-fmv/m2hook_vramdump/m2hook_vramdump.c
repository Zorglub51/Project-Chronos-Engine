/*
 * m2hook_vramdump.c — Periodically dump m2engage's emulated VDC VRAM.
 *
 * Two-part hook (SPEC §4):
 *   (a) One-shot inline patch of the VDC constructor at VMA 0x7aa08 to capture
 *       the VDC context pointer (passed in r0).
 *   (b) Background pthread that wakes every ~2 s, reads the VRAM pointer from
 *       *(g_vdc_ctx + 0x14), and dumps the 64 KB VRAM to /tmp/m2_vram_NNNN.bin
 *       (rotating to ~20 files), with a one-line summary in /tmp/m2_vramdump.log.
 *
 * The constructor target is one-shot per game-instance and the dumper is
 * background — no high-rate patched site, so no VFP-save scaffolding needed
 * (cf. m2hook_cdtrace's lesson — that hook patches a debug logger called from
 * FP-using code at log-rate; we don't).
 *
 * Cross-compile (Mini target, A33 / Cortex-A7 armhf):
 *   bash build.sh
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
#include <pthread.h>
#include <sys/mman.h>
#include <sys/stat.h>

#define LOG_PATH       "/tmp/m2_vramdump.log"
#define DUMP_DIR       "/tmp"
#define DUMP_PREFIX    "m2_vram_"
#define VRAM_SIZE      0x10000u           /* 64 KB */
#define VDC_HEADER_LOG 256                /* bytes of *g_vdc_ctx to log */
#define DUMP_PERIOD_US (2 * 1000 * 1000)  /* 2 s */
#define DUMP_KEEP      20                  /* rotate after this many files */

/* SPEC §4a signature — unique in the stock m2engage. */
static const uint8_t g_signature[16] = {
    0x2d, 0xe9, 0xf0, 0x4f, 0x07, 0x46, 0x87, 0xb0,
    0x08, 0x46, 0x4c, 0xf2, 0xc4, 0x21, 0x03, 0xaa,
};
/* First 8 bytes — displaced instructions to replay. */
static const uint8_t g_expected_prologue[8] = {
    0x2d, 0xe9, 0xf0, 0x4f, 0x07, 0x46, 0x87, 0xb0,
};

/* Shared with the asm trampoline. */
uint32_t g_resume_addr;          /* (ctor_addr + 8) | 1 */

/* State. */
static FILE              *g_log;
static volatile uint32_t  g_vdc_ctx;
static volatile int       g_armed;
static unsigned           g_dump_seq;
static pthread_t          g_dumper_tid;

/* ---- C handler — called from the trampoline with the constructor's r0
 *      (= the VDC context pointer) preserved. ---- */
/* Forward — dumper thread body. */
static void *dumper_thread(void *arg);

/* m2engage fork()s before the VDC constructor runs. Our LD_PRELOAD
 * constructor runs in the PARENT (a setup process); the emulator code
 * path runs in the CHILD (different memory space). Threads do NOT
 * carry into the child via fork. So we (re-)spawn the dumper inside
 * the handler, which runs in the child where g_vdc_ctx actually gets
 * populated. The flag is per-process — each child gets its own thread. */
static int g_dumper_spawned;

static void spawn_dumper_once(void)
{
    int expected = 0;
    if (!__atomic_compare_exchange_n(&g_dumper_spawned, &expected, 1,
                                      0, __ATOMIC_ACQ_REL, __ATOMIC_RELAXED)) {
        return;
    }
    pthread_t tid;
    if (pthread_create(&tid, NULL, dumper_thread, NULL) == 0) {
        pthread_detach(tid);
        if (g_log) {
            fprintf(g_log,
                "[VRAM] dumper thread launched in pid %d (post-fork)\n",
                (int)getpid());
            fflush(g_log);
        }
    } else if (g_log) {
        fprintf(g_log, "[VRAM] ERROR: pthread_create failed: %s\n",
                strerror(errno));
        fflush(g_log);
    }
}

void vdcctor_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    (void)r1; (void)r2; (void)r3;
    __atomic_store_n(&g_vdc_ctx, r0, __ATOMIC_RELEASE);
    if (g_log) {
        fprintf(g_log, "[VRAM] VDC ctor: ctx=0x%08x (pid %d)\n",
                r0, (int)getpid());
        fflush(g_log);
    }
    spawn_dumper_once();
}

/* ---- Trampoline (Thumb naked) ----
 *
 * Layout (matches m2hook_cdtrace.c shape, adapted for 8 displaced bytes that
 * cover 3 instructions instead of 4):
 *   1. push  {r0-r3, r12, lr}    ; preserve constructor args
 *   2. bl    vdcctor_handler      ; receives r0 = VDC ctx intact
 *   3. pop   {r0-r3, r12, lr}
 *   4. replay 3 displaced instructions (8 bytes total):
 *        stmdb sp!, {r4,r5,r6,r7,r8,r9,sl,fp,lr}     ; e92d 4ff0
 *        mov   r7, r0                                 ; 4607
 *        sub   sp, #28                                ; b087
 *   5. ldr   r12, =g_resume_addr ; ldr r12,[r12] ; bx r12
 *
 * No VFP save needed — this site fires at most a handful of times per process
 * lifetime (once per emulator-context creation), not at log/sample rate.
 */
__attribute__((naked, used, target("thumb")))
void vdcctor_trampoline(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        /* Save args + AAPCS caller-saved scratch. */
        "push {r0, r1, r2, r3, r12, lr}\n"

        /* Call C handler — r0..r3 already in place. */
        "bl   vdcctor_handler\n"

        /* Restore. */
        "pop  {r0, r1, r2, r3, r12, lr}\n"

        /* Replay 3 displaced instructions from 0x7aa08..0x7aa0f.
         * NOTE: stmdb sp!, {...} encodes as a 32-bit Thumb-2 instruction
         * (e92d 4ff0). The assembler will emit it correctly. */
        "stmdb sp!, {r4, r5, r6, r7, r8, r9, sl, fp, lr}\n"
        "mov   r7, r0\n"
        "sub   sp, #28\n"

        /* Resume at ctor_addr + 8 with Thumb bit set. */
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

/* ---- Pointer sanity ---- */

static int plausible_heap_ptr(uint32_t p)
{
    /* On the Mini m2engage maps libc heap roughly in 0x00400000..0x10000000.
     * Be conservative: require non-tiny and not obviously kernel-space. */
    if (p < 0x10000u) return 0;
    if (p >= 0xC0000000u) return 0;
    return 1;
}

/* ---- VRAM byte census ---- */

static uint32_t count_nonzero(const uint8_t *p, size_t n)
{
    uint32_t c = 0;
    for (size_t i = 0; i < n; i++) if (p[i]) c++;
    return c;
}

/* ---- Dump file helpers ---- */

static void make_dump_path(char *out, size_t outsz, unsigned idx)
{
    snprintf(out, outsz, "%s/%s%04u.bin", DUMP_DIR, DUMP_PREFIX, idx);
}

static void rotate_old_dumps(unsigned current_idx)
{
    if (current_idx < DUMP_KEEP) return;
    char path[128];
    make_dump_path(path, sizeof(path), current_idx - DUMP_KEEP);
    unlink(path);
}

/* Write 'n' bytes from 'p' to 'path'. Returns 0 on success. */
static int dump_to_file(const char *path, const void *p, size_t n)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return -1;
    size_t written = 0;
    while (written < n) {
        ssize_t w = write(fd, (const char *)p + written, n - written);
        if (w < 0) { close(fd); return -1; }
        written += (size_t)w;
    }
    close(fd);
    return 0;
}

/* ---- Background dumper thread ---- */

static void *dumper_thread(void *arg)
{
    (void)arg;
    unsigned heartbeat = 0;
    for (;;) {
        /* Use nanosleep — POSIX usleep is capped at < 1 000 000 us on
         * older glibcs (the Mini runs Linux 3.4-era glibc) and returns
         * EINVAL above that, leaving the loop spinning. nanosleep takes
         * arbitrary durations. */
        struct timespec ts = { .tv_sec = 2, .tv_nsec = 0 };
        nanosleep(&ts, NULL);

        uint32_t ctx = __atomic_load_n(&g_vdc_ctx, __ATOMIC_ACQUIRE);

        if (g_log && (heartbeat++ % 5) == 0) {
            fprintf(g_log, "[VRAM] heartbeat #%u ctx=0x%08x\n",
                    heartbeat, (unsigned)ctx);
            fflush(g_log);
        }
        if (!ctx) {
            /* Constructor hasn't fired yet. Quiet wait. */
            continue;
        }

        /* Read VRAM pointer from VDC ctx + 0x14. The pointer may shift across
         * game launches as the engine re-allocs — read it fresh each tick. */
        uint32_t vram_ptr = *(volatile uint32_t *)(uintptr_t)(ctx + 0x14);
        if (!plausible_heap_ptr(vram_ptr)) {
            if (g_log) {
                fprintf(g_log,
                    "[VRAM] tick: ctx=0x%08x vram_ptr=0x%08x — not plausible, skipping\n",
                    ctx, vram_ptr);
                fflush(g_log);
            }
            continue;
        }

        const uint8_t *vram = (const uint8_t *)(uintptr_t)vram_ptr;
        unsigned idx = g_dump_seq++;

        /* Census + dump. */
        uint32_t nonzero = count_nonzero(vram, VRAM_SIZE);

        char path[128];
        make_dump_path(path, sizeof(path), idx);
        int rc = dump_to_file(path, vram, VRAM_SIZE);

        /* Also dump the VDC ctx header to a sibling file — same index. */
        char ctx_path[128];
        snprintf(ctx_path, sizeof(ctx_path),
                 "%s/m2_vdcctx_%04u.bin", DUMP_DIR, idx);
        (void)dump_to_file(ctx_path, (const void *)(uintptr_t)ctx,
                           VDC_HEADER_LOG);

        if (g_log) {
            time_t now = time(NULL);
            struct tm *tm = localtime(&now);
            char ts[32];
            strftime(ts, sizeof(ts), "%H:%M:%S", tm);
            fprintf(g_log,
                "[VRAM] #%04u %s ctx=0x%08x vram=0x%08x nonzero=%u/%u (%.1f%%) -> %s%s\n",
                idx, ts, ctx, vram_ptr,
                nonzero, VRAM_SIZE,
                100.0 * (double)nonzero / (double)VRAM_SIZE,
                path, rc == 0 ? "" : " (write failed)");
            fflush(g_log);
        }

        rotate_old_dumps(idx);
    }
    return NULL;
}

/* ---- atexit ---- */

static void atexit_summary(void)
{
    if (!g_log) return;
    fprintf(g_log, "[VRAM] === exiting, total dumps=%u ===\n", g_dump_seq);
    fflush(g_log);
}

/* ---- Constructor ---- */

__attribute__((constructor))
static void m2hook_vramdump_init(void)
{
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) {
        return;
    }

    g_log = fopen(LOG_PATH, "a");
    if (!g_log) {
        fprintf(stderr, "[VRAM] cannot open %s: %s\n", LOG_PATH, strerror(errno));
        return;
    }
    setvbuf(g_log, NULL, _IOLBF, 0);

    time_t now = time(NULL);
    fprintf(g_log, "\n[VRAM] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);
    fprintf(g_log, "[VRAM] time: %s", ctime(&now));

    /* Locate m2engage code mapping + signature. */
    uintptr_t code_addr = 0;
    size_t    code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(g_log, "[VRAM] ERROR: could not find m2engage r-xp mapping\n");
        return;
    }
    fprintf(g_log, "[VRAM] code section: 0x%08x .. 0x%08x (%zu bytes)\n",
            (unsigned)code_addr, (unsigned)(code_addr + code_size), code_size);

    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) {
        fprintf(g_log, "[VRAM] ERROR: VDC ctor signature not found\n");
        return;
    }
    uintptr_t ctor_addr = (uintptr_t)match;
    fprintf(g_log, "[VRAM] VDC ctor found at 0x%08x\n", (unsigned)ctor_addr);

    if (memcmp(match, g_expected_prologue, 8) != 0) {
        fprintf(g_log, "[VRAM] ERROR: prologue mismatch — wrong binary?\n");
        return;
    }

    g_resume_addr = (uint32_t)(ctor_addr + 8) | 1u;

    /* Patch: mprotect RWX, write redirect, restore RX. */
    uintptr_t page_start = ctor_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(g_log, "[VRAM] ERROR: mprotect RWX failed: %s\n", strerror(errno));
        return;
    }

    uint16_t *thumb = (uint16_t *)match;
    uint32_t  hook_addr = (uint32_t)(uintptr_t)vdcctor_trampoline;

    thumb[0] = 0xF8DF;
    thumb[1] = 0xF000;
    thumb[2] = (uint16_t)(hook_addr & 0xFFFF);
    thumb[3] = (uint16_t)((hook_addr >> 16) & 0xFFFF);

    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)match, (char *)match + 8);

    g_armed = 1;
    fprintf(g_log,
        "[VRAM] hook armed at 0x%08x -> trampoline 0x%08x (resume 0x%08x)\n",
        (unsigned)ctor_addr, hook_addr, (unsigned)g_resume_addr);
    fprintf(g_log,
        "[VRAM] constructor pid %d — dumper will spawn on first VDC ctor "
        "fire (post-fork, in the emulator process)\n",
        (int)getpid());
    fflush(g_log);

    atexit(atexit_summary);
}
