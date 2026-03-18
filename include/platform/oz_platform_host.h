/* Platform Abstraction Layer — Host (POSIX / C11) backend */
#ifndef OZ_PLATFORM_HOST_H
#define OZ_PLATFORM_HOST_H

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdatomic.h>
#include "oz_platform_types.h"

/* ------------------------------------------------------------------ */
/* Slab allocator — malloc-backed with block-count tracking            */
/* ------------------------------------------------------------------ */

struct oz_slab {
        size_t block_size;
        uint32_t num_blocks;
        uint32_t num_used;
};

typedef struct oz_slab oz_slab_t;

#define OZ_SLAB_DEFINE(name, blk_size, n_blocks, alignment)                    \
        oz_slab_t name = {                                                     \
                .block_size = (blk_size),                                      \
                .num_blocks = (n_blocks),                                      \
                .num_used = 0                                                  \
        }

static inline int oz_slab_alloc(oz_slab_t *slab, void **mem)
{
        if (slab->num_used >= slab->num_blocks) {
                *mem = NULL;
                return OZ_ENOMEM;
        }
        *mem = malloc(slab->block_size);
        if (!*mem) {
                return OZ_ENOMEM;
        }
        slab->num_used++;
        return OZ_OK;
}

static inline void oz_slab_free(oz_slab_t *slab, void *mem)
{
        free(mem);
        if (slab->num_used > 0) {
                slab->num_used--;
        }
}

/* ------------------------------------------------------------------ */
/* Contiguous block allocator — malloc-backed for OZArray/OZDictionary */
/* ------------------------------------------------------------------ */

struct oz_mem_blocks {
        size_t block_size;
        uint32_t num_blocks;
        uint32_t num_used;
};

typedef struct oz_mem_blocks oz_mem_blocks_t;

#define OZ_MEM_BLOCKS_DEFINE(name, blk_size, n_blocks, alignment)              \
        oz_mem_blocks_t name = {                                               \
                .block_size = (blk_size),                                      \
                .num_blocks = (n_blocks),                                      \
                .num_used = 0                                                  \
        }

static inline int oz_mem_blocks_alloc_contiguous(oz_mem_blocks_t *pool,
                                                 uint32_t count, void **mem)
{
        if (pool->num_used + count > pool->num_blocks) {
                *mem = NULL;
                return OZ_ENOMEM;
        }
        *mem = malloc(pool->block_size * count);
        if (!*mem) {
                return OZ_ENOMEM;
        }
        pool->num_used += count;
        return OZ_OK;
}

static inline void oz_mem_blocks_free_contiguous(oz_mem_blocks_t *pool,
                                                 void *mem, uint32_t count)
{
        free(mem);
        if (pool->num_used >= count) {
                pool->num_used -= count;
        }
}

/* ------------------------------------------------------------------ */
/* Atomic integers — C11 stdatomic                                     */
/* ------------------------------------------------------------------ */

typedef _Atomic(int) oz_atomic_t;

static inline void oz_atomic_init(oz_atomic_t *target, int val)
{
        atomic_store(target, val);
}

static inline int oz_atomic_inc(oz_atomic_t *target)
{
        return atomic_fetch_add(target, 1) + 1;
}

static inline bool oz_atomic_dec_and_test(oz_atomic_t *target)
{
        return atomic_fetch_sub(target, 1) == 1;
}

static inline int oz_atomic_get(oz_atomic_t *target)
{
        return atomic_load(target);
}

/* ------------------------------------------------------------------ */
/* Spinlock — no-op on host (single-threaded tests)                    */
/* ------------------------------------------------------------------ */

typedef int oz_spinlock_t;
typedef int oz_spinlock_key_t;
#define OZ_SPINLOCK(lck) if ((void)(lck), 1)

static inline oz_spinlock_key_t oz_spin_lock(oz_spinlock_t *lck)
{
        (void)lck;
        return 0;
}

static inline void oz_spin_unlock(oz_spinlock_t *lck, oz_spinlock_key_t key)
{
        (void)lck;
        (void)key;
}

/* ------------------------------------------------------------------ */
/* Formatted output — printf                                           */
/* ------------------------------------------------------------------ */

#define oz_platform_print(fmt, ...) printf(fmt, ##__VA_ARGS__)
#define oz_platform_snprint(buf, len, fmt, ...) snprintf(buf, len, fmt, ##__VA_ARGS__)

/* ------------------------------------------------------------------ */
/* Auto-initialization — constructor attribute for +initialize methods */
/* ------------------------------------------------------------------ */

#define OZ_AUTO_INIT(fn_name, init_fn)                                           \
        __attribute__((constructor))                                              \
        static void fn_name(void) { init_fn(); }

#endif /* OZ_PLATFORM_HOST_H */
