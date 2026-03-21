/* Platform Abstraction Layer — shared types */
#ifndef OZ_PLATFORM_TYPES_H
#define OZ_PLATFORM_TYPES_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#ifndef OZ_OK
#define OZ_OK 0
#endif

#ifndef OZ_ENOMEM
#define OZ_ENOMEM (-12)
#endif

/* ------------------------------------------------------------------ */
/* Object metadata — bitfield replacing the former oz_class_id enum   */
/* ------------------------------------------------------------------ */

/**
 * @brief Per-object metadata packed into a single 32-bit word.
 *
 * Layout (LSB first):
 *   [0:9]  class_id      — dispatch table index (max 1024 classes)
 *   [10]   heap_allocated — object lives in an OZHeap / system heap
 *   [11]   deallocating   — re-entrant dealloc guard
 *   [12]   immortal       — skip dealloc (singletons, literals)
 *   [13:31] reserved
 */
struct oz_metadata {
        uint32_t class_id        : 10;
        uint32_t heap_allocated  :  1;
        uint32_t deallocating    :  1;
        uint32_t immortal        :  1;
        uint32_t reserved        : 19;
};

#endif /* OZ_PLATFORM_TYPES_H */
