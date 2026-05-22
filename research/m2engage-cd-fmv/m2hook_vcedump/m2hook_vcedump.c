/*
 * m2hook_vcedump.c — Periodically dump m2engage's emulated VCE CRAM (palette).
 *
 * Small extension of m2hook_vramdump:
 *   - same one-shot inline patch of the VDC/VCE constructor at VMA 0x7aa08
 *     (the engine merges VDC + VCE into one module — the context carries
 *      both buffers, see m2hook_vramdump/REPORT.md);
 *   - same lazy-spawn background thread (post-fork — vramdump's lesson);
 *   - per tick, dumps the CRAM at *(VDC_ctx + 0x18) — 1024 bytes — and
 *     logs the non-zero count (so "palette all-black during cutscene" is
 *     visible at a glance).
 *
 * CRAM offset derivation: disassembly of the VDC/VCE ctor (0x7aa08..0x7ac80)
 * shows four bl 0x713c0 state-save registration calls. The second one passes
 * r1 = *(r7 + 0x18) and r3 = 512 (= 512 × 16-bit entries = 1024 bytes), which
 * matches the PCE VCE CRAM (32 palettes × 16 entries × 9-bit, stored as
 * 16-bit). VRAM (+0x14, 64 KB) is the first call; cram (+0x18, 1 KB) is the
 * second.
 *
 * Cross-compile for the Mini (A33 Cortex-A7 armhf):
 *   bash build.sh
 *
 * SEE ALSO: SPEC §4b proposes a per-frame VCE-port-write trace
 * (ports $0402-$0405) for the "rare flash" investigation. That requires
 * additional static RE to locate the I/O handler and isn't implemented
 * here — this hook is the periodic CRAM buffer dump from §4, which the
 * spec explicitly allows to run alongside as corroboration.
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

#define LOG_PATH       "/tmp/m2_vcedump.log"
#define DUMP_DIR       "/tmp"
#define DUMP_PREFIX    "m2_cram_"
#define CRAM_SIZE      1024u                /* 512 entries × 16-bit */
#define VDC_HEADER_LOG 256
#define DUMP_PERIOD_MS 500                  /* dump 2× per second — CRAM is 1 KB; cheap */
#define DUMP_KEEP      120                  /* ≈ 60 s of history at 500 ms */

#define CRAM_OFFSET    0x18                 /* CRAM ptr offset inside VDC ctx */

/* SPEC §4a signature — unique in the stock m2engage (same as vramdump). */
static const uint8_t g_signature[16] = {
    0x2d, 0xe9, 0xf0, 0x4f, 0x07, 0x46, 0x87, 0xb0,
    0x08, 0x46, 0x4c, 0xf2, 0xc4, 0x21, 0x03, 0xaa,
};
static const uint8_t g_expected_prologue[8] = {
    0x2d, 0xe9, 0xf0, 0x4f, 0x07, 0x46, 0x87, 0xb0,
};

uint32_t g_resume_addr;          /* (ctor_addr + 8) | 1 */

static FILE              *g_log;
static volatile uint32_t  g_vdc_ctx;
static int                g_armed;
static unsigned           g_dump_seq;
static int                g_dumper_spawned;

static void *dumper_thread(void *arg);

/* Lazy-spawn dumper post-fork (vramdump's lesson). */
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
                "[VCE] dumper thread launched in pid %d (post-fork)\n",
                (int)getpid());
            fflush(g_log);
        }
    } else if (g_log) {
        fprintf(g_log, "[VCE] ERROR: pthread_create failed: %s\n",
                strerror(errno));
        fflush(g_log);
    }
}

void vcector_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    (void)r1; (void)r2; (void)r3;
    __atomic_store_n(&g_vdc_ctx, r0, __ATOMIC_RELEASE);
    if (g_log) {
        fprintf(g_log, "[VCE] VDC/VCE ctor: ctx=0x%08x (pid %d)\n",
                r0, (int)getpid());
        fflush(g_log);
    }
    spawn_dumper_once();
}

/* ---- Trampoline (Thumb naked) — identical to vramdump's ----
 *
 * Same patched function (VDC/VCE ctor at 0x7aa08), same 3 displaced
 * instructions. The C handler we call here is different (vcector_handler);
 * everything else carries forward unchanged.
 */
__attribute__((naked, used, target("thumb")))
void vcector_trampoline(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        "push {r0, r1, r2, r3, r12, lr}\n"
        "bl   vcector_handler\n"
        "pop  {r0, r1, r2, r3, r12, lr}\n"

        "stmdb sp!, {r4, r5, r6, r7, r8, r9, sl, fp, lr}\n"
        "mov   r7, r0\n"
        "sub   sp, #28\n"

        "ldr  r12, =g_resume_addr\n"
        "ldr  r12, [r12]\n"
        "bx   r12\n"

        ".ltorg\n"
    );
}

/* ---- Pattern scan in the m2engage r-x mapping ---- */

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

static int plausible_heap_ptr(uint32_t p)
{
    if (p < 0x10000u) return 0;
    if (p >= 0xC0000000u) return 0;
    return 1;
}

static uint32_t count_nonzero(const uint8_t *p, size_t n)
{
    uint32_t c = 0;
    for (size_t i = 0; i < n; i++) if (p[i]) c++;
    return c;
}

static uint32_t count_nonzero_words(const uint16_t *p, size_t nwords)
{
    uint32_t c = 0;
    for (size_t i = 0; i < nwords; i++) if (p[i]) c++;
    return c;
}

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

static void *dumper_thread(void *arg)
{
    (void)arg;
    unsigned heartbeat = 0;
    for (;;) {
        struct timespec ts = {
            .tv_sec  = DUMP_PERIOD_MS / 1000,
            .tv_nsec = (DUMP_PERIOD_MS % 1000) * 1000000L,
        };
        nanosleep(&ts, NULL);

        uint32_t ctx = __atomic_load_n(&g_vdc_ctx, __ATOMIC_ACQUIRE);

        if (g_log && (heartbeat++ % 20) == 0) {
            fprintf(g_log, "[VCE] heartbeat #%u ctx=0x%08x\n",
                    heartbeat, (unsigned)ctx);
            fflush(g_log);
        }

        if (!ctx) continue;

        /* CRAM pointer at VDC ctx + 0x18. */
        uint32_t cram_ptr = *(volatile uint32_t *)(uintptr_t)(ctx + CRAM_OFFSET);
        if (!plausible_heap_ptr(cram_ptr)) {
            if (g_log) {
                fprintf(g_log,
                    "[VCE] tick: ctx=0x%08x cram_ptr=0x%08x — not plausible, skipping\n",
                    ctx, cram_ptr);
                fflush(g_log);
            }
            continue;
        }

        const uint8_t  *cram  = (const uint8_t  *)(uintptr_t)cram_ptr;
        const uint16_t *cramw = (const uint16_t *)(uintptr_t)cram_ptr;
        unsigned idx = g_dump_seq++;

        uint32_t nonzero_b = count_nonzero(cram, CRAM_SIZE);
        uint32_t nonzero_w = count_nonzero_words(cramw, CRAM_SIZE / 2);

        char path[128];
        make_dump_path(path, sizeof(path), idx);
        int rc = dump_to_file(path, cram, CRAM_SIZE);

        char ctx_path[128];
        snprintf(ctx_path, sizeof(ctx_path),
                 "%s/m2_vcectx_%04u.bin", DUMP_DIR, idx);
        (void)dump_to_file(ctx_path, (const void *)(uintptr_t)ctx,
                           VDC_HEADER_LOG);

        if (g_log) {
            time_t now = time(NULL);
            struct tm *tm = localtime(&now);
            char tsbuf[32];
            strftime(tsbuf, sizeof(tsbuf), "%H:%M:%S", tm);

            /* First 8 16-bit entries (palette 0, entries 0..7) — a "head"
             * preview so you can see at-a-glance whether anything's there. */
            fprintf(g_log,
                "[VCE] #%04u %s ctx=0x%08x cram=0x%08x "
                "nonzero_bytes=%u/%u (%.1f%%) "
                "nonzero_entries=%u/512 "
                "head=[%04x %04x %04x %04x %04x %04x %04x %04x] -> %s%s\n",
                idx, tsbuf, ctx, cram_ptr,
                nonzero_b, CRAM_SIZE,
                100.0 * (double)nonzero_b / (double)CRAM_SIZE,
                nonzero_w,
                cramw[0], cramw[1], cramw[2], cramw[3],
                cramw[4], cramw[5], cramw[6], cramw[7],
                path, rc == 0 ? "" : " (write failed)");
            fflush(g_log);
        }

        rotate_old_dumps(idx);
    }
    return NULL;
}

static void atexit_summary(void)
{
    if (!g_log) return;
    fprintf(g_log, "[VCE] === exiting, total dumps=%u ===\n", g_dump_seq);
    fflush(g_log);
}

__attribute__((constructor))
static void m2hook_vcedump_init(void)
{
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) {
        return;
    }

    g_log = fopen(LOG_PATH, "a");
    if (!g_log) {
        fprintf(stderr, "[VCE] cannot open %s: %s\n", LOG_PATH, strerror(errno));
        return;
    }
    setvbuf(g_log, NULL, _IOLBF, 0);

    time_t now = time(NULL);
    fprintf(g_log, "\n[VCE] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);
    fprintf(g_log, "[VCE] time: %s", ctime(&now));

    uintptr_t code_addr = 0;
    size_t    code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(g_log, "[VCE] ERROR: could not find m2engage r-xp mapping\n");
        return;
    }
    fprintf(g_log, "[VCE] code section: 0x%08x .. 0x%08x (%zu bytes)\n",
            (unsigned)code_addr, (unsigned)(code_addr + code_size), code_size);

    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) {
        fprintf(g_log, "[VCE] ERROR: VDC/VCE ctor signature not found\n");
        return;
    }
    uintptr_t ctor_addr = (uintptr_t)match;
    fprintf(g_log, "[VCE] VDC/VCE ctor found at 0x%08x\n", (unsigned)ctor_addr);

    if (memcmp(match, g_expected_prologue, 8) != 0) {
        fprintf(g_log, "[VCE] ERROR: prologue mismatch — wrong binary?\n");
        return;
    }

    g_resume_addr = (uint32_t)(ctor_addr + 8) | 1u;

    uintptr_t page_start = ctor_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(g_log, "[VCE] ERROR: mprotect RWX failed: %s\n", strerror(errno));
        return;
    }

    uint16_t *thumb = (uint16_t *)match;
    uint32_t  hook_addr = (uint32_t)(uintptr_t)vcector_trampoline;

    thumb[0] = 0xF8DF;
    thumb[1] = 0xF000;
    thumb[2] = (uint16_t)(hook_addr & 0xFFFF);
    thumb[3] = (uint16_t)((hook_addr >> 16) & 0xFFFF);

    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)match, (char *)match + 8);

    g_armed = 1;
    fprintf(g_log,
        "[VCE] hook armed at 0x%08x -> trampoline 0x%08x (resume 0x%08x)\n",
        (unsigned)ctor_addr, hook_addr, (unsigned)g_resume_addr);
    fprintf(g_log,
        "[VCE] constructor pid %d — dumper will spawn on first ctor fire "
        "(post-fork, in the emulator process). CRAM offset = +0x%02x, "
        "period = %d ms, keep last %d.\n",
        (int)getpid(), CRAM_OFFSET, DUMP_PERIOD_MS, DUMP_KEEP);
    fflush(g_log);

    atexit(atexit_summary);
}
