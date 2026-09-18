#ifndef CHRONOS_FRAME_PACING_H
#define CHRONOS_FRAME_PACING_H
#include <stdint.h>

#define CHRONOS_FRAME_NS (INT64_C(1000000000) / 60)

/* Advance the scheduled deadline, not the actual (possibly late) wake time.
 * Bound catch-up to one frame after a long stall or a suspended VM. */
static inline int64_t frame_next_deadline(int64_t previous, int64_t now)
{
    int64_t next = previous + CHRONOS_FRAME_NS;
    return now - next > CHRONOS_FRAME_NS ? now : next;
}
#endif
