#include <assert.h>
#include <stdio.h>
#include "graphics/frame_pacing.h"

int main(void)
{
    /* A VM waking 0.75 ms late must still produce 600 frames in 10 s,
     * rather than accumulating 450 ms of slowdown and starving audio. */
    int64_t deadline = 0, wake = 0;
    for (int frame = 1; frame <= 600; ++frame) {
        int64_t render_done = wake + 2000000;
        deadline = frame_next_deadline(deadline, render_done);
        assert(deadline == frame * CHRONOS_FRAME_NS);
        assert(deadline > render_done);
        wake = deadline + 750000;
    }
    assert(wake - 600 * CHRONOS_FRAME_NS == 750000);

    /* A single moderately late frame catches up on the existing timeline. */
    int64_t late = deadline + CHRONOS_FRAME_NS + 3000000;
    int64_t next = frame_next_deadline(deadline, late);
    assert(next == deadline + CHRONOS_FRAME_NS);
    assert(frame_next_deadline(next, late + 2000000) > late + 2000000);

    /* After a long pause, discard the backlog instead of fast-forwarding. */
    late = deadline + INT64_C(5000000000);
    next = frame_next_deadline(deadline, late);
    assert(next == late);
    assert(frame_next_deadline(next, late + 2000000) == late + CHRONOS_FRAME_NS);

    /* Deadlines remain monotonic across timespec second boundaries. */
    assert(frame_next_deadline(INT64_C(999999999), INT64_C(1000000001))
           == INT64_C(999999999) + CHRONOS_FRAME_NS);
    puts("Frame pacing: oversleep does not drift; long-stall catch-up is bounded");
    return 0;
}
