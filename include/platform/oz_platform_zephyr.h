/* Platform Abstraction Layer — Zephyr RTOS backend */
#ifndef OZ_PLATFORM_ZEPHYR_H
#define OZ_PLATFORM_ZEPHYR_H

#include <zephyr/kernel.h>
#include <zephyr/sys/atomic.h>
#include <zephyr/sys/printk.h>
#include <zephyr/sys/mem_blocks.h>
#include "oz_platform_types.h"

/* ------------------------------------------------------------------ */
/* Slab allocator — k_mem_slab pass-through                            */
/* ------------------------------------------------------------------ */

typedef struct k_mem_slab oz_slab_t;

#define OZ_SLAB_DEFINE(name, blk_size, n_blocks, alignment)                    \
        K_MEM_SLAB_DEFINE(name, blk_size, n_blocks, alignment)

static inline int oz_slab_alloc(oz_slab_t *slab, void **mem)
{
        return k_mem_slab_alloc(slab, mem, K_NO_WAIT);
}

static inline void oz_slab_free(oz_slab_t *slab, void *mem)
{
        k_mem_slab_free(slab, mem);
}

/* ------------------------------------------------------------------ */
/* Contiguous block allocator — sys_mem_blocks pass-through            */
/* ------------------------------------------------------------------ */

typedef struct sys_mem_blocks oz_mem_blocks_t;

#define OZ_MEM_BLOCKS_DEFINE(name, blk_size, n_blocks, alignment)              \
        SYS_MEM_BLOCKS_DEFINE(name, blk_size, n_blocks, alignment)

static inline int oz_mem_blocks_alloc_contiguous(oz_mem_blocks_t *pool,
                                                 uint32_t count, void **mem)
{
        return sys_mem_blocks_alloc_contiguous(pool, count, mem);
}

static inline void oz_mem_blocks_free_contiguous(oz_mem_blocks_t *pool,
                                                 void *mem, uint32_t count)
{
        sys_mem_blocks_free_contiguous(pool, mem, count);
}

/* ------------------------------------------------------------------ */
/* Atomic integers — Zephyr atomic wrappers                            */
/* ------------------------------------------------------------------ */

typedef atomic_t oz_atomic_t;

static inline void oz_atomic_init(oz_atomic_t *target, atomic_val_t val)
{
        atomic_set(target, val);
}

static inline atomic_val_t oz_atomic_inc(oz_atomic_t *target)
{
        return atomic_inc(target) + 1;
}

static inline bool oz_atomic_dec_and_test(oz_atomic_t *target)
{
        return atomic_dec(target) == 1;
}

static inline atomic_val_t oz_atomic_get(oz_atomic_t *target)
{
        return atomic_get(target);
}

/* ------------------------------------------------------------------ */
/* Spinlock — scoped preemption guard for atomic property accessors     */
/* ------------------------------------------------------------------ */

typedef struct k_spinlock oz_spinlock_t;
typedef k_spinlock_key_t oz_spinlock_key_t;
#define OZ_SPINLOCK(lck) K_SPINLOCK(lck)

static inline oz_spinlock_key_t oz_spin_lock(oz_spinlock_t *lck)
{
        return k_spin_lock(lck);
}

static inline void oz_spin_unlock(oz_spinlock_t *lck, oz_spinlock_key_t key)
{
        k_spin_unlock(lck, key);
}

/* ------------------------------------------------------------------ */
/* Formatted output — printk                                           */
/* ------------------------------------------------------------------ */

#define oz_platform_print(fmt, ...) printk(fmt, ##__VA_ARGS__)
#define oz_platform_snprint(buf, len, fmt, ...) snprintk(buf, len, fmt, ##__VA_ARGS__)

#endif /* OZ_PLATFORM_ZEPHYR_H */
