/* Checked positive Squirrel argument indices, independent of pointer width.
 * The retail VM stores absolute _top/_stackbase as signed32 integers. */
#ifndef CHRONOS_SQ_STRING_ARG_H
#define CHRONOS_SQ_STRING_ARG_H
#include <stdint.h>
static int chronos_sq_argument_slot(int32_t base, int32_t top, int32_t index, int32_t *slot)
{
    if (!slot || base < 0 || top < base || index <= 0 || index > top - base) return 0;
    /* The comparison above guarantees this addition cannot overflow and
     * cannot address an old value beyond the active stack's top. */
    *slot = base + index - 1;
    return 1;
}
#endif
