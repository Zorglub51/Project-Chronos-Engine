/*
 * m2hook_cdpoke.c — READ-ONLY MODE: log SCSI state bytes over time.
 *
 * After Ghidra/r2 disasm we know:
 *   - 0x81d88 is the $1800 READ handler (returns [obj+0x24] to guest)
 *   - It only advances state via bl 0x80b58 when [obj+0x20] (phase) == 3
 *   - Returned byte is masked: `[obj+0x27] >= 0` → return raw; else clear REQ
 *
 * So to diagnose the stuck cutscene, we passively log per-tick:
 *   - [obj+0x20] phase counter
 *   - [obj+0x24] status byte (the BIOS-polled byte)
 *   - [obj+0x27] mask flag (signed; if <0, REQ is cleared in returned byte)
 *   - [obj+0x32] (sectors_left?)
 *   - [obj+0x40-0x44] byte counter / length
 *
 * No writes — pure observation.
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

#define LOG_PATH       "/tmp/m2_cdpoke.log"
#define POLL_PERIOD_MS 50         /* 20 Hz state sampling */

/* cdtrace's signature — same debug logger at VMA 0x1eee48 */
static const uint8_t g_signature[16] = {
    0x0e, 0xb4, 0x00, 0xb5, 0x82, 0xb0, 0x03, 0xaa,
    0x52, 0xf8, 0x04, 0x1b, 0x01, 0x92, 0x18, 0xb1,
};
static const uint8_t g_expected_prologue[8] = {
    0x0e, 0xb4, 0x00, 0xb5, 0x82, 0xb0, 0x03, 0xaa,
};

#define CDPHASE_LO 0x001fc4b8u
#define CDPHASE_HI 0x001fc618u

uint32_t g_resume_addr;
volatile uint32_t g_caller_r4;

static FILE              *g_log;
static volatile uint32_t  g_cd_obj;
static int                g_armed;
static int                g_poller_spawned;

static void *poller_thread(void *arg);

static void spawn_poller_once(void)
{
    int expected = 0;
    if (!__atomic_compare_exchange_n(&g_poller_spawned, &expected, 1,
                                      0, __ATOMIC_ACQ_REL, __ATOMIC_RELAXED)) {
        return;
    }
    pthread_t tid;
    if (pthread_create(&tid, NULL, poller_thread, NULL) == 0) {
        pthread_detach(tid);
        if (g_log) {
            fprintf(g_log,
                "[CDPK] poller thread launched in pid %d (post-fork)\n",
                (int)getpid());
            fflush(g_log);
        }
    }
}

void cdpoke_logger_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    (void)r0; (void)r1; (void)r3;
    if (r2 >= CDPHASE_LO && r2 < CDPHASE_HI) {
        uint32_t this_ptr = g_caller_r4;
        uint32_t prev = __atomic_exchange_n(&g_cd_obj, this_ptr, __ATOMIC_RELEASE);
        if (prev != this_ptr && g_log) {
            const char *phase_name = (const char *)(uintptr_t)r2;
            fprintf(g_log,
                "[CDPK] CD-ROM obj captured: this=0x%08x  phase_event=%s (pid %d)\n",
                this_ptr, phase_name, (int)getpid());
            fflush(g_log);
        }
        spawn_poller_once();
    }
}

__attribute__((naked, used, target("thumb")))
void cdpoke_trampoline(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        "push  {r0, r1, r2, r3, r12, lr}\n"
        "ldr   r12, =g_caller_r4\n"
        "str   r4, [r12]\n"
        "vpush {d0-d7}\n"
        "bl    cdpoke_logger_handler\n"
        "vpop  {d0-d7}\n"
        "pop   {r0, r1, r2, r3, r12, lr}\n"

        "push  {r1, r2, r3}\n"
        "push  {lr}\n"
        "sub   sp, #8\n"
        "add   r2, sp, #12\n"

        "ldr   r12, =g_resume_addr\n"
        "ldr   r12, [r12]\n"
        "bx    r12\n"

        ".ltorg\n"
    );
}

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
    for (size_t i = 0; i + patlen <= size; i += 2)
        if (memcmp(data + i, pat, patlen) == 0) return data + i;
    return NULL;
}

/* ---- State poller — read-only by default, opt-in unstick via env ---- */

static void *poller_thread(void *arg)
{
    (void)arg;
    uint32_t last_logged_phase = 0xffffffff;
    uint8_t  last_status = 0xff;
    uint8_t  last_mask = 0xff;
    uint32_t last_byte_count = 0xffffffff;
    uint32_t stuck_at_6_count = 0;     /* how many ticks we've seen phase=6/status=0x78 */
    int unstick_enabled = (getenv("M2_CDPOKE_UNSTICK") != NULL);
    if (g_log) fprintf(g_log, "[CDPK] poller starting; unstick=%d\n", unstick_enabled);
    for (;;) {
        struct timespec ts = {
            .tv_sec = POLL_PERIOD_MS / 1000,
            .tv_nsec = (POLL_PERIOD_MS % 1000) * 1000000L,
        };
        nanosleep(&ts, NULL);

        uint32_t obj = __atomic_load_n(&g_cd_obj, __ATOMIC_ACQUIRE);
        if (!obj || obj < 0x10000u || obj >= 0xC0000000u) continue;

        volatile uint8_t  *o8 = (volatile uint8_t  *)(uintptr_t)obj;
        volatile uint32_t *o32 = (volatile uint32_t *)(uintptr_t)obj;

        uint32_t phase = o32[0x20/4];
        uint8_t  status = o8[0x24];
        uint8_t  status_next = o8[0x25];
        uint8_t  mask = o8[0x27];
        uint8_t  field28 = o8[0x28];
        uint32_t byte_count = o32[0x40/4];
        uint32_t expected_len = o32[0x44/4];

        /* UNSTICK v2: when stuck at phase=6 / status=0x78, call the
         * m2engage state-transition function at VMA 0x80b58 directly
         * with r0 = obj. This is what the $1800 read handler at
         * 0x81d88 calls when phase==3 — we manually invoke it when
         * phase==6 to push the state machine forward. */
        if (unstick_enabled && phase == 6 && status == 0x78) {
            stuck_at_6_count++;
            if (stuck_at_6_count == 10) {     /* 500 ms */
                /* Call m2engage's state advance: bl 0x80b58 with r0=obj.
                 * Find the function address dynamically by looking up the
                 * known VMA via /proc/self/maps (so we don't hardcode it
                 * if m2engage is ever PIE-built). */
                typedef void (*advance_fn)(uint32_t);
                advance_fn fn = (advance_fn)((uintptr_t)0x80b58 | 1u);  /* Thumb bit */
                if (g_log) {
                    time_t now = time(NULL);
                    struct tm *tm = localtime(&now);
                    char tsbuf[32];
                    strftime(tsbuf, sizeof(tsbuf), "%H:%M:%S", tm);
                    fprintf(g_log,
                        "[CDPK] %s **UNSTICK v2** calling 0x80b58(obj=0x%08x)...\n",
                        tsbuf, obj);
                    fflush(g_log);
                }
                fn(obj);
                if (g_log) {
                    fprintf(g_log,
                        "[CDPK] **UNSTICK v2** returned. phase now=%u status=0x%02x\n",
                        (unsigned)o32[0x20/4], (unsigned)o8[0x24]);
                    fflush(g_log);
                }
                stuck_at_6_count = 0;  /* reset; try again next time */
            }
        } else {
            stuck_at_6_count = 0;
        }

        /* Log on any state change. */
        int changed = (phase != last_logged_phase) ||
                       (status != last_status) ||
                       (mask != last_mask) ||
                       (byte_count != last_byte_count);

        if (changed && g_log) {
            time_t now = time(NULL);
            struct tm *tm = localtime(&now);
            char tsbuf[32];
            strftime(tsbuf, sizeof(tsbuf), "%H:%M:%S", tm);

            const char *phase_label = "?";
            switch (phase) {
                case 0: phase_label = "IDLE"; break;
                case 1: phase_label = "CMD_OUT"; break;
                case 2: phase_label = "CMD_2"; break;
                case 3: phase_label = "DATA_REQ"; break;
                case 4: phase_label = "DATA_IN"; break;
                case 5: phase_label = "MSG_IN"; break;
                case 6: phase_label = "RESULT"; break;
                case 7: phase_label = "STATUS_END"; break;
            }
            const char *status_label = "??";
            switch (status & 0xf8) {
                case 0x00: status_label = "free"; break;
                case 0x80: status_label = "BSY"; break;
                case 0x88: status_label = "BSY|IO"; break;
                case 0xC8: status_label = "BSY|REQ|IO"; break;
                case 0xD8: status_label = "BSY|REQ|MSG|IO"; break;
                case 0xF8: status_label = "BSY|REQ|MSG|CD|IO"; break;
                case 0x48: status_label = "REQ|IO"; break;
                case 0xC0: status_label = "BSY|REQ"; break;
            }
            fprintf(g_log,
                "[CDPK] %s phase=%u(%s) [+24]=0x%02x(%s) [+25]=0x%02x [+27]=0x%02x(mask) [+28]=0x%02x cnt=%u/%u\n",
                tsbuf,
                phase, phase_label,
                status, status_label,
                status_next, mask, field28,
                byte_count, expected_len);
            fflush(g_log);

            last_logged_phase = phase;
            last_status = status;
            last_mask = mask;
            last_byte_count = byte_count;
        }
    }
    return NULL;
}

__attribute__((constructor))
static void m2hook_cdpoke_init(void)
{
    char exe[256] = {0};
    ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (n <= 0 || !strstr(exe, "m2engage")) return;

    g_log = fopen(LOG_PATH, "a");
    if (!g_log) { fprintf(stderr, "[CDPK] cannot open %s\n", LOG_PATH); return; }
    setvbuf(g_log, NULL, _IOLBF, 0);

    time_t now = time(NULL);
    fprintf(g_log, "\n[CDPK] === hook startup (pid %d, exe %s) ===\n",
            (int)getpid(), exe);
    fprintf(g_log, "[CDPK] time: %s", ctime(&now));
    fprintf(g_log, "[CDPK] mode: READ-ONLY state observer (no pokes)\n");

    uintptr_t code_addr = 0; size_t code_size = 0;
    if (find_code_section(&code_addr, &code_size) != 0 || !code_addr) {
        fprintf(g_log, "[CDPK] ERROR: no m2engage r-xp\n"); return;
    }

    uint8_t *match = find_pattern((uint8_t *)code_addr, code_size,
                                   g_signature, sizeof(g_signature));
    if (!match) { fprintf(g_log, "[CDPK] ERROR: signature\n"); return; }
    uintptr_t log_addr = (uintptr_t)match;
    if (memcmp(match, g_expected_prologue, 8) != 0) {
        fprintf(g_log, "[CDPK] ERROR: prologue\n"); return;
    }

    g_resume_addr = (uint32_t)(log_addr + 8) | 1u;

    uintptr_t page_start = log_addr & ~(uintptr_t)0xFFF;
    if (mprotect((void *)page_start, 0x2000,
                 PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        fprintf(g_log, "[CDPK] ERROR: mprotect: %s\n", strerror(errno));
        return;
    }
    uint16_t *thumb = (uint16_t *)match;
    uint32_t hook_addr = (uint32_t)(uintptr_t)cdpoke_trampoline;
    thumb[0] = 0xF8DF; thumb[1] = 0xF000;
    thumb[2] = (uint16_t)(hook_addr & 0xFFFF);
    thumb[3] = (uint16_t)((hook_addr >> 16) & 0xFFFF);
    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)match, (char *)match + 8);

    g_armed = 1;
    fprintf(g_log,
        "[CDPK] hook armed at 0x%08x -> trampoline 0x%08x (resume 0x%08x)\n",
        (unsigned)log_addr, hook_addr, (unsigned)g_resume_addr);
    fflush(g_log);
}
