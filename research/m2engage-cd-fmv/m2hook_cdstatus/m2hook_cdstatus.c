/*
 * m2hook_cdstatus.c — Capture m2engage's CD-ROM emulator object pointer at
 * its constructor, then periodically dump the SCSI flag bytes to disk.
 *
 * Background:
 *   PopfulMail's BIOS cutscene-streaming routine at $EABE polls CD_STATUS
 *   ($1800) waiting for 0xC8 (BSY|REQ|IO) to read each chunk's bytes. m2engage
 *   gets through SOME chunks but then stalls — the BIOS loop never sees the
 *   next 0xC8 transition. We need to see m2engage's SCSI signal state during
 *   the stuck period.
 *
 *   Geargrafx's get_cdrom_status reports: scsi_bsy, scsi_req, scsi_io,
 *   scsi_cd, scsi_msg as individual flag fields. m2engage stores these as
 *   byte fields inside the CD-ROM emulator object. The status byte returned
 *   to the guest is composed from these flags.
 *
 *   Hook the CD-ROM ctor at VMA 0x7fc40 (signature scanned, unique). The C
 *   handler captures r0 = `this` (the CD-ROM emulator object). A background
 *   thread then dumps a 256-byte window starting from `this` every ~50 ms,
 *   logging the bytes that change between samples.
 *
 * Patched site = ctor entry, called once per game launch — no high-rate
 * concerns, no VFP scaffolding needed (cdtrace's lesson).
 *
 * Lessons applied from previous hooks:
 *   - nanosleep over usleep (vramdump)
 *   - spawn dumper thread post-fork via atomic-CAS once-flag (vramdump)
 *   - atomic acquire/release on the shared ctx pointer (vramdump)
 *   - the patched function's prologue straddles a 4-byte instruction, so we
 *     replay 4 instructions (10 bytes) in the trampoline and resume at +10.
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

#define LOG_PATH       "/tmp/m2_cdstatus.log"
#define DUMP_DIR       "/tmp"
#define DUMP_PREFIX    "m2_cdobj_"
#define CDOBJ_SIZE     256                  /* bytes of CD-ROM object to dump */
#define DUMP_PERIOD_MS 50                   /* 20 Hz sampling */
#define DUMP_KEEP      400                  /* ~20 s of history at 50 ms */

/* CD-ROM ctor signature — verified unique at file offset 0x77c40 (VMA 0x7fc40). */
static const uint8_t g_signature[16] = {
    0xf0, 0xb5, 0x0e, 0x46, 0x85, 0xb0, 0x4c, 0xf2,
    0x74, 0x61, 0xc0, 0xf2, 0x1f, 0x01, 0x04, 0x46,
};
/* First 8 bytes — what we'll overwrite. Straddles the movw at offset +6. */
static const uint8_t g_expected_prologue[8] = {
    0xf0, 0xb5, 0x0e, 0x46, 0x85, 0xb0, 0x4c, 0xf2,
};

/* Resume after the 4 displaced instructions (push, mov, sub, movw) = 10 bytes. */
uint32_t g_resume_addr;

static FILE              *g_log;
static volatile uint32_t  g_cd_ctx;
static int                g_armed;
static unsigned           g_dump_seq;
static int                g_dumper_spawned;

static void *dumper_thread(void *arg);

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
                "[CDST] dumper thread launched in pid %d (post-fork)\n",
                (int)getpid());
            fflush(g_log);
        }
    } else if (g_log) {
        fprintf(g_log, "[CDST] ERROR: pthread_create failed: %s\n",
                strerror(errno));
        fflush(g_log);
    }
}

void cdrom_ctor_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    (void)r1; (void)r2; (void)r3;
    /* r0 at the CD-ROM ctor entry = `this` (CD-ROM emulator object). */
    __atomic_store_n(&g_cd_ctx, r0, __ATOMIC_RELEASE);
    if (g_log) {
        fprintf(g_log, "[CDST] CD-ROM ctor: this=0x%08x (pid %d)\n",
                r0, (int)getpid());
        fflush(g_log);
    }
    spawn_dumper_once();
}

/* ---- Trampoline (Thumb naked) ----
 *
 * The 8-byte LDR.W PC redirect straddles the start of a 4-byte movw
 * instruction at the patch site. Replay all 4 original instructions in the
 * trampoline (= 10 bytes), then resume at original_addr + 10.
 *
 * Original prologue (from disasm at 0x7fc40):
 *   0x7fc40: f0 b5      push {r4, r5, r6, r7, lr}
 *   0x7fc42: 0e 46      mov  r6, r1
 *   0x7fc44: 85 b0      sub  sp, #20
 *   0x7fc46: 4c f2 74 61   movw r1, #0xc674
 *   0x7fc4a: ...        (next instruction — resume here)
 */
__attribute__((naked, used, target("thumb")))
void cdrom_ctor_trampoline(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        /* Save dispatcher args + AAPCS caller-saved. */
        "push {r0, r1, r2, r3, r12, lr}\n"

        /* Call C handler with args (r0,r1,r2,r3) intact. */
        "bl   cdrom_ctor_handler\n"

        /* Restore. */
        "pop  {r0, r1, r2, r3, r12, lr}\n"

        /* Replay the 4 displaced instructions. */
        "push {r4, r5, r6, r7, lr}\n"
        "mov   r6, r1\n"
        "sub   sp, #20\n"
        "movw  r1, #0xc674\n"

        /* Resume at ctor_addr + 10 with Thumb bit set. */
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

static int plausible_heap_ptr(uint32_t p)
{
    if (p < 0x10000u) return 0;
    if (p >= 0xC0000000u) return 0;
    return 1;
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
    uint8_t prev_buf[CDOBJ_SIZE] = {0};
    int prev_valid = 0;
    for (;;) {
        struct timespec ts = {
            .tv_sec  = DUMP_PERIOD_MS / 1000,
            .tv_nsec = (DUMP_PERIOD_MS % 1000) * 1000000L,
        };
        nanosleep(&ts, NULL);

        uint32_t ctx = __atomic_load_n(&g_cd_ctx, __ATOMIC_ACQUIRE);

        if (g_log && (heartbeat++ % 100) == 0) {
            fprintf(g_log, "[CDST] heartbeat #%u ctx=0x%08x\n",
                    heartbeat, (unsigned)ctx);
            fflush(g_log);
        }

        if (!ctx || !plausible_heap_ptr(ctx)) continue;

        const uint8_t *cd = (const uint8_t *)(uintptr_t)ctx;
        unsigned idx = g_dump_seq++;

        /* Dump full 256-byte window. */
        char path[128];
        make_dump_path(path, sizeof(path), idx);
        int rc = dump_to_file(path, cd, CDOBJ_SIZE);

        /* Log only if any byte in the SCSI-flag region (offsets 0x18..0x60)
         * changed since the previous sample. Captures every transition
         * without flooding the log when state is idle. */
        int changed = 0;
        if (prev_valid) {
            for (size_t i = 0x18; i < 0x60; i++) {
                if (cd[i] != prev_buf[i]) { changed = 1; break; }
            }
        } else {
            changed = 1;
        }

        if (g_log && changed) {
            time_t now = time(NULL);
            struct tm *tm = localtime(&now);
            char tsbuf[32];
            strftime(tsbuf, sizeof(tsbuf), "%H:%M:%S", tm);
            /* Print SCSI-relevant bytes inline: offsets 0x18..0x40 covers the
             * fields registered in the ctor. */
            fprintf(g_log, "[CDST] #%04u %s ctx=0x%08x",
                    idx, tsbuf, ctx);
            fprintf(g_log, "  scsi[0x18..0x3f]:");
            for (int o = 0x18; o < 0x40; o++) {
                fprintf(g_log, " %02x", cd[o]);
            }
            fprintf(g_log, "%s\n", rc == 0 ? "" : " (write failed)");
            fflush(g_log);
            memcpy(prev_buf, cd, CDOBJ_SIZE);
            prev_valid = 1;
        }

        rotate_old_dumps(idx);
    }
    return NULL;
}

static void atexit_summary(void)
{
    if (!g_log) return;
    fprintf(g_log, "[CDST] === exiting, total dumps=%u ===\n", g_dump_seq);
    fflush(g_log);
}

__attribute__((constructor))
static void m2hook_cdstatus_init(void)
{
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) {
        return;
    }

    g_log = fopen(LOG_PATH, "a");
    if (!g_log) {
        fprintf(stderr, "[CDST] cannot open %s: %s\n", LOG_PATH, strerror(errno));
        return;
    }
    setvbuf(g_log, NULL, _IOLBF, 0);

    time_t now = time(NULL);
    fprintf(g_log, "\n[CDST] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);
    fprintf(g_log, "[CDST] time: %s", ctime(&now));

    uintptr_t code_addr = 0;
    size_t    code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(g_log, "[CDST] ERROR: could not find m2engage r-xp mapping\n");
        return;
    }
    fprintf(g_log, "[CDST] code section: 0x%08x .. 0x%08x (%zu bytes)\n",
            (unsigned)code_addr, (unsigned)(code_addr + code_size), code_size);

    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) {
        fprintf(g_log, "[CDST] ERROR: CD-ROM ctor signature not found\n");
        return;
    }
    uintptr_t ctor_addr = (uintptr_t)match;
    fprintf(g_log, "[CDST] CD-ROM ctor found at 0x%08x\n", (unsigned)ctor_addr);

    if (memcmp(match, g_expected_prologue, 8) != 0) {
        fprintf(g_log, "[CDST] ERROR: prologue mismatch — wrong binary?\n");
        return;
    }

    /* Resume after 4 displaced instructions = 10 bytes. */
    g_resume_addr = (uint32_t)(ctor_addr + 10) | 1u;

    uintptr_t page_start = ctor_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(g_log, "[CDST] ERROR: mprotect RWX failed: %s\n", strerror(errno));
        return;
    }

    uint16_t *thumb = (uint16_t *)match;
    uint32_t  hook_addr = (uint32_t)(uintptr_t)cdrom_ctor_trampoline;

    /* LDR.W PC, [PC, #0] ; .word trampoline_addr — 8 bytes total. */
    thumb[0] = 0xF8DF;
    thumb[1] = 0xF000;
    thumb[2] = (uint16_t)(hook_addr & 0xFFFF);
    thumb[3] = (uint16_t)((hook_addr >> 16) & 0xFFFF);

    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)match, (char *)match + 8);

    g_armed = 1;
    fprintf(g_log,
        "[CDST] hook armed at 0x%08x -> trampoline 0x%08x (resume 0x%08x)\n",
        (unsigned)ctor_addr, hook_addr, (unsigned)g_resume_addr);
    fprintf(g_log,
        "[CDST] period=%d ms, keep=%d dumps, will log on SCSI-byte changes\n",
        DUMP_PERIOD_MS, DUMP_KEEP);
    fflush(g_log);

    atexit(atexit_summary);
}
