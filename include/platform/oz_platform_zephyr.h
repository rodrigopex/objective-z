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

/* ------------------------------------------------------------------ */
/* Heap allocator — sys_heap + spinlock wrapper for allocWithHeap:     */
/* ------------------------------------------------------------------ */

#ifdef OZ_HEAP_SUPPORT
#include <zephyr/sys/sys_heap.h>
#include <zephyr/sys/mem_stats.h>
#define OZ_HEAP_INNER_DEFINED

/**
 * @brief Platform-specific heap inner type (Zephyr).
 *
 * Wraps sys_heap + spinlock for thread-safe heap allocation.
 */
struct oz_heap_inner {
        struct sys_heap heap;
        struct k_spinlock lock;
};

/**
 * @brief Heap allocation header — placed before heap-allocated objects.
 *
 * Use CONTAINER_OF(obj, struct oz_heap_hdr, obj) to recover the header
 * and find which heap the object belongs to.
 */
struct OZHeap;

struct oz_heap_hdr {
        struct OZHeap *heap;
        size_t alloc_size;
        char obj[];
};

static inline void oz_heap_init(struct oz_heap_inner *inner,
                                void *buf, size_t size)
{
        sys_heap_init(&inner->heap, buf, size);
}

static inline void *oz_heap_alloc_obj(struct oz_heap_inner *inner,
                                      struct OZHeap *owner, size_t size)
{
        size_t total = sizeof(struct oz_heap_hdr) + size;
        k_spinlock_key_t key = k_spin_lock(&inner->lock);
        void *raw = sys_heap_alloc(&inner->heap, total);
        k_spin_unlock(&inner->lock, key);
        if (!raw) {
                return NULL;
        }
        struct oz_heap_hdr *hdr = (struct oz_heap_hdr *)raw;
        hdr->heap = owner;
        hdr->alloc_size = total;
        return hdr->obj;
}

static inline void oz_heap_free_obj(struct oz_heap_inner *inner, void *obj)
{
        struct oz_heap_hdr *hdr = (struct oz_heap_hdr *)
                ((char *)obj - offsetof(struct oz_heap_hdr, obj));
        k_spinlock_key_t key = k_spin_lock(&inner->lock);
        sys_heap_free(&inner->heap, hdr);
        k_spin_unlock(&inner->lock, key);
}

static inline void *oz_sys_heap_alloc(size_t size)
{
        size_t total = sizeof(struct oz_heap_hdr) + size;
        void *raw = k_malloc(total);
        if (!raw) {
                return NULL;
        }
        struct oz_heap_hdr *hdr = (struct oz_heap_hdr *)raw;
        hdr->heap = NULL;
        hdr->alloc_size = total;
        return hdr->obj;
}

static inline void oz_sys_heap_free(void *obj)
{
        struct oz_heap_hdr *hdr = (struct oz_heap_hdr *)
                ((char *)obj - offsetof(struct oz_heap_hdr, obj));
        k_free(hdr);
}

/**
 * @brief Query user heap allocated bytes via sys_heap_runtime_stats.
 *
 * Requires CONFIG_SYS_HEAP_RUNTIME_STATS=y (auto-selected by CONFIG_OBJZ_HEAP).
 */
static inline size_t oz_heap_used_bytes(struct oz_heap_inner *inner)
{
        struct sys_memory_stats stats;
        if (sys_heap_runtime_stats_get(&inner->heap, &stats) == 0) {
                return stats.allocated_bytes;
        }
        return 0;
}

/**
 * @brief Allocate from an OZHeap or system heap.
 * @brief Free a heap-allocated object (resolves heap via CONTAINER_OF).
 *
 * Defined in the generated oz_dispatch.c — requires struct OZHeap
 * to be complete, which is only guaranteed after all class headers
 * have been included.
 */
void *oz_heap_obj_alloc(struct OZHeap *heap, size_t size);
void oz_heap_obj_free(void *obj);

#endif /* OZ_HEAP_SUPPORT */

/* ------------------------------------------------------------------ */
/* Timer helper — bridges void* to k_timer function pointers           */
/* ------------------------------------------------------------------ */

/*
 * __oz_timer_setup — ARC forbids direct block-to-fptr cast;
 * void*-to-fptr is unrestricted. Bridges the gap for k_timer_init.
 */
static inline void __oz_timer_setup(struct k_timer *t, void *exp,
                                     void *stp, void *ud)
{
        k_timer_init(t, (k_timer_expiry_t)exp, (k_timer_stop_t)stp);
        k_timer_user_data_set(t, ud);
}

/* ------------------------------------------------------------------ */
/* Auto-initialization — SYS_INIT for +initialize methods              */
/* ------------------------------------------------------------------ */

#include <zephyr/init.h>

#define OZ_AUTO_INIT(fn_name, init_fn)                                           \
        static int fn_name(void) { init_fn(); return 0; }                        \
        SYS_INIT(fn_name, APPLICATION, 90)

#endif /* OZ_PLATFORM_ZEPHYR_H */
