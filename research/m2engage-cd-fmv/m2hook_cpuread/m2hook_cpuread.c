/*
 * m2hook_cpuread.c — Log every guest read of port $1800 (CD_STATUS).
 *
 * Hooks m2engage's HuC6280 CPU read dispatcher at VMA 0x81b40. That function
 * is called on every guest memory read — millions of times per second. We
 * filter in the asm trampoline before saving any state: cmp r1 (= guest
 * address) against #0x1800; if not equal, bypass the C handler entirely and
 * just replay the displaced instructions. The fast path is ~3 instructions
 * of overhead per non-matching read.
 *
 * When r1 == 0x1800, the C handler increments a 32-bit counter and (optionally)
 * appends to a small ring buffer so a background thread can drain to a log
 * file. We don't capture the returned byte at this hook (it's not yet
 * computed at function entry); the cdtrace log + the timestamped poll rate
 * is enough diagnostic data to correlate with the SCSI phase machine.
 *
 * Dispatcher signature (verified unique):
 *   4b 0b 0a 46 7f 2b f0 b5 04 46 83 b0 1e d9 f7 2b
 *
 * The 4 displaced instructions (8 bytes):
 *   0x81b40:  4b 0b     lsrs r3, r1, #13       (bank index)
 *   0x81b42:  0a 46     mov  r2, r1
 *   0x81b44:  7f 2b     cmp  r3, #127
 *   0x81b46:  f0 b5     push {r4-r7, lr}
 * Resume at 0x81b48.
 *
 * High-rate site: must vpush {d0-d7} around the C call (cdtrace's lesson).
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

#define LOG_PATH       "/tmp/m2_cpuread.log"
#define DRAIN_PERIOD_MS 1000      /* 1 Hz drain — write summary stats every sec */
#define EVENT_BUF_SIZE  4096      /* ring buffer capacity (in entries) */

/* Signature for the CPU read dispatcher at VMA 0x81b40 — verified unique. */
static const uint8_t g_signature[16] = {
    0x4b, 0x0b, 0x0a, 0x46, 0x7f, 0x2b, 0xf0, 0xb5,
    0x04, 0x46, 0x83, 0xb0, 0x1e, 0xd9, 0xf7, 0x2b,
};
/* First 8 bytes — what we overwrite. Exactly 4 complete Thumb instructions. */
static const uint8_t g_expected_prologue[8] = {
    0x4b, 0x0b, 0x0a, 0x46, 0x7f, 0x2b, 0xf0, 0xb5,
};

/* Resume at dispatcher_addr + 8 (= 0x81b48 with Thumb bit set). */
uint32_t g_resume_addr;

static FILE     *g_log;
static int       g_armed;

/* Counters / event buffer. */
static volatile uint64_t g_total_reads;
static volatile uint64_t g_1800_reads;
static struct event {
    uint32_t  ts_us;   /* low 32 bits of monotonic μs */
} g_events[EVENT_BUF_SIZE];
static volatile uint32_t g_event_write_pos;    /* incremented on each event */
static volatile uint32_t g_event_drain_pos;    /* increments as drained */

static uint32_t now_us_low32(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    uint64_t us = (uint64_t)ts.tv_sec * 1000000ULL + (uint64_t)ts.tv_nsec / 1000ULL;
    return (uint32_t)us;
}

/* m2engage fork()s. The LD_PRELOAD constructor runs in the parent; the
 * emulator (and therefore $1800 reads) happens in the child. We must
 * spawn the drainer thread in the child — vramdump/vcedump's lesson. */
static int g_drainer_spawned;
static void *drain_thread(void *arg);

static void spawn_drainer_once(void)
{
    int expected = 0;
    if (!__atomic_compare_exchange_n(&g_drainer_spawned, &expected, 1,
                                      0, __ATOMIC_ACQ_REL, __ATOMIC_RELAXED)) {
        return;
    }
    pthread_t tid;
    if (pthread_create(&tid, NULL, drain_thread, NULL) == 0) {
        pthread_detach(tid);
        if (g_log) {
            fprintf(g_log,
                "[CPURD] drainer thread launched in pid %d (post-fork)\n",
                (int)getpid());
            fflush(g_log);
        }
    }
}

/* The C handler runs ONLY when r1==0x1800 (the asm filters it). Keep it
 * minimal — increment counter, record timestamp into ring buffer.
 * First call also lazy-spawns the drainer in this process. */
void cpuread_1800_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    (void)r0; (void)r1; (void)r2; (void)r3;
    __atomic_fetch_add(&g_1800_reads, 1, __ATOMIC_RELAXED);

    uint32_t pos = __atomic_fetch_add(&g_event_write_pos, 1, __ATOMIC_RELAXED);
    g_events[pos % EVENT_BUF_SIZE].ts_us = now_us_low32();

    /* Cheap check after increment — once-flag is RELAXED-load fast path. */
    if (!__atomic_load_n(&g_drainer_spawned, __ATOMIC_RELAXED)) {
        spawn_drainer_once();
    }
}

/* ---- Trampoline (Thumb naked) ----
 *
 * Fast path: cmp r1, #0x1800; bne replay. If r1 != 0x1800, skip all the
 * save/call/restore and go straight to replay+resume.
 *
 * Hot path (r1==0x1800): full save, C handler call, restore.
 */
__attribute__((naked, used, target("thumb")))
void cpuread_trampoline(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        /* TEMPORARY: filter on $2000 (WRAM base) — very frequently
         * read by guest CPU for stack/data ops. ~kHz of hits expected.
         * If we get hits, dispatcher works; change back to #0x1800.
         * If 0 hits, m2engage HLE-bypasses this dispatcher entirely. */
        "cmp.w r1, #0x2000\n"
        "bne   1f\n"

        /* Match — full save (incl. VFP — high-rate site, cdtrace lesson). */
        "push   {r0, r1, r2, r3, r12, lr}\n"
        "vpush  {d0-d7}\n"
        "bl     cpuread_1800_handler\n"
        "vpop   {d0-d7}\n"
        "pop    {r0, r1, r2, r3, r12, lr}\n"

        /* Replay the 4 displaced original instructions. */
        "1:\n"
        "lsrs  r3, r1, #13\n"
        "mov   r2, r1\n"
        "cmp   r3, #127\n"
        "push  {r4, r5, r6, r7, lr}\n"

        /* Resume at dispatcher_addr + 8 with Thumb bit set. */
        "ldr   r12, =g_resume_addr\n"
        "ldr   r12, [r12]\n"
        "bx    r12\n"

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

/* ---- Background drainer thread ---- */

static void *drain_thread(void *arg)
{
    (void)arg;
    uint64_t last_count = 0;
    for (;;) {
        struct timespec ts = {
            .tv_sec  = DRAIN_PERIOD_MS / 1000,
            .tv_nsec = (DRAIN_PERIOD_MS % 1000) * 1000000L,
        };
        nanosleep(&ts, NULL);

        if (!g_log) continue;
        uint64_t now_count = __atomic_load_n(&g_1800_reads, __ATOMIC_RELAXED);
        if (now_count == last_count) {
            /* Silent period — don't spam. Heartbeat every ~10s. */
            continue;
        }
        uint64_t delta = now_count - last_count;
        last_count = now_count;

        /* Drain ring buffer — only the entries newer than last drain. */
        uint32_t writepos = __atomic_load_n(&g_event_write_pos, __ATOMIC_ACQUIRE);
        uint32_t drainpos = g_event_drain_pos;
        uint32_t to_drain = writepos - drainpos;
        /* Cap at one buffer size; older entries are clobbered. */
        if (to_drain > EVENT_BUF_SIZE) {
            drainpos = writepos - EVENT_BUF_SIZE;
            to_drain = EVENT_BUF_SIZE;
        }

        time_t now = time(NULL);
        struct tm *tm = localtime(&now);
        char tsbuf[32];
        strftime(tsbuf, sizeof(tsbuf), "%H:%M:%S", tm);

        fprintf(g_log,
            "[CPURD] %s  $1800-reads-this-sec=%lu  total=%lu  buffered=%u\n",
            tsbuf, (unsigned long)delta, (unsigned long)now_count, to_drain);

        /* Print inter-arrival time for the last few hits to detect polling
         * rhythm. */
        if (to_drain > 0 && to_drain <= 10) {
            fprintf(g_log, "[CPURD]   recent inter-arrivals (μs):");
            uint32_t prev_ts = 0;
            for (uint32_t i = 0; i < to_drain; i++) {
                uint32_t idx = (drainpos + i) % EVENT_BUF_SIZE;
                uint32_t cur_ts = g_events[idx].ts_us;
                if (i > 0) {
                    fprintf(g_log, " %u", cur_ts - prev_ts);
                }
                prev_ts = cur_ts;
            }
            fprintf(g_log, "\n");
        }
        fflush(g_log);
        g_event_drain_pos = writepos;
    }
    return NULL;
}

/* ---- Constructor ---- */

__attribute__((constructor))
static void m2hook_cpuread_init(void)
{
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) {
        return;
    }

    g_log = fopen(LOG_PATH, "a");
    if (!g_log) {
        fprintf(stderr, "[CPURD] cannot open %s: %s\n", LOG_PATH, strerror(errno));
        return;
    }
    setvbuf(g_log, NULL, _IOLBF, 0);

    time_t now = time(NULL);
    fprintf(g_log, "\n[CPURD] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);
    fprintf(g_log, "[CPURD] time: %s", ctime(&now));

    uintptr_t code_addr = 0;
    size_t    code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(g_log, "[CPURD] ERROR: could not find m2engage r-xp mapping\n");
        return;
    }
    fprintf(g_log, "[CPURD] code section: 0x%08x .. 0x%08x (%zu bytes)\n",
            (unsigned)code_addr, (unsigned)(code_addr + code_size), code_size);

    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) {
        fprintf(g_log, "[CPURD] ERROR: dispatcher signature not found\n");
        return;
    }
    uintptr_t disp_addr = (uintptr_t)match;
    fprintf(g_log, "[CPURD] dispatcher found at 0x%08x\n", (unsigned)disp_addr);

    if (memcmp(match, g_expected_prologue, 8) != 0) {
        fprintf(g_log, "[CPURD] ERROR: prologue mismatch — wrong binary?\n");
        return;
    }

    g_resume_addr = (uint32_t)(disp_addr + 8) | 1u;

    uintptr_t page_start = disp_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(g_log, "[CPURD] ERROR: mprotect RWX failed: %s\n", strerror(errno));
        return;
    }

    uint16_t *thumb = (uint16_t *)match;
    uint32_t  hook_addr = (uint32_t)(uintptr_t)cpuread_trampoline;

    thumb[0] = 0xF8DF;
    thumb[1] = 0xF000;
    thumb[2] = (uint16_t)(hook_addr & 0xFFFF);
    thumb[3] = (uint16_t)((hook_addr >> 16) & 0xFFFF);

    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)match, (char *)match + 8);

    g_armed = 1;
    fprintf(g_log,
        "[CPURD] hook armed at 0x%08x -> trampoline 0x%08x (resume 0x%08x)\n",
        (unsigned)disp_addr, hook_addr, (unsigned)g_resume_addr);
    fprintf(g_log,
        "[CPURD] filter: r1 == 0x1800 (CD_STATUS); fast-path bypasses for all other reads\n");
    fflush(g_log);

    /* Drainer thread is spawned lazily from the C handler on first
     * $1800 read — that way it lives in the forked child where the
     * actual emulation runs. Same vramdump lesson. */
}
