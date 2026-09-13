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
#ifdef OZ_DEBUG_REFCOUNT
        /* The clamp below is the third thing that makes a double free
         * invisible, after the refcount reaching -1 silently and the dealloc
         * switch's `default:` arm breaking without a word: `num_used` is
         * already 0, so a second free changes nothing and no count is ever
         * wrong afterwards (#452).
         *
         * Host only, and deliberately. Zephyr's `oz_slab_free` is a
         * pass-through to `k_mem_slab_free`, which keeps its own accounting
         * and has no `num_used` here to underflow -- there is nothing at this
         * layer to assert about on that backend. */
        oz_assert_msg(slab->num_used > 0,
                      "slab free with no outstanding allocation -- this slot was freed "
                      "twice, or returned to the wrong slab");
#endif
        if (slab->num_used > 0) {
                slab->num_used--;
        }
}

/* ------------------------------------------------------------------ */
/* Slab leak detection — check for outstanding allocations at exit     */
/* ------------------------------------------------------------------ */

static inline uint32_t oz_slab_outstanding_count(oz_slab_t *slab)
{
        return slab->num_used;
}

static inline int oz_slab_check_leaks(oz_slab_t *slab, const char *name)
{
        if (slab->num_used > 0) {
                fprintf(stderr, "LEAK: %s has %u outstanding allocation(s)\n",
                        name, slab->num_used);
                return 1;
        }
        return 0;
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

/**
 * Self-terminating, like the Zephyr backend's SYS_MEM_BLOCKS_DEFINE:
 * write it without a trailing `;`. The two backends have to agree on
 * that, because generated code is written once and compiled against
 * both -- and Zephyr's own macro carries the `;` inside its body, so a
 * call site that adds one leaves a bare `;` at file scope there, an
 * empty declaration and a constraint violation (#266).
 */
#define OZ_MEM_BLOCKS_DEFINE(name, blk_size, n_blocks, alignment)              \
        oz_mem_blocks_t name = {                                               \
                .block_size = (blk_size),                                      \
                .num_blocks = (n_blocks),                                      \
                .num_used = 0                                                  \
        };

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

/** @brief Zero a spinlock before first use -- see the Zephyr backend for
 *  why generated code calls this instead of using a brace initializer. */
static inline void oz_spin_init(oz_spinlock_t *lck)
{
        *lck = 0;
}

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

/**
 * @brief A zeroed lock key, for a variable that may never be assigned one.
 *
 * Trivial here, where the key is a scalar, but it must exist on both backends:
 * on Zephyr `oz_spinlock_key_t` is a struct and `= 0` does not compile. Kept
 * as a memset for the same reason `oz_spin_init` is one -- so the spelling
 * does not depend on the type being scalar.
 */
static inline oz_spinlock_key_t oz_spin_key_none(void)
{
        oz_spinlock_key_t k;

        memset(&k, 0, sizeof(k));
        return k;
}

/**
 * @brief Identity of the calling thread, for re-entrancy detection.
 *
 * The host backend is single threaded, so one constant identity is the
 * truth. It must be non-NULL and stable: generated code compares it against
 * an object's recorded owner, which is zero when the lock is free, so a NULL
 * identity would make a free lock look like one this thread already holds
 * and the acquire would be skipped.
 *
 * Returning a constant is also what makes the re-entrancy path testable on
 * host at all: `@synchronized(x) { @synchronized(x) { } }` takes the
 * skip-the-second-acquire branch here exactly as it does on Zephyr, even
 * though the spinlock itself is a no-op.
 */
static inline void *oz_current_thread(void)
{
        return (void *)1;
}

/* ------------------------------------------------------------------ */
/* Formatted output — printf                                           */
/* ------------------------------------------------------------------ */

/*
 * `(...)` forwarding the whole list, not `(fmt, ...)` with `, ##__VA_ARGS__`.
 * The `##` form is a GNU extension for swallowing the comma when no
 * variadic argument is given, and clang diagnoses it at *every* expansion
 * under `-std=c17 -pedantic-errors` -- not only the zero-argument ones:
 *
 *     error: token pasting of ',' and __VA_ARGS__ is a GNU extension
 *            [-Wgnu-zero-variadic-macro-arguments]
 *
 * That went unnoticed because nothing expanded these macros. #452's
 * refcount traps are the first generated C to call `OZ_PLATFORM_PRINT`, and
 * they made a latent non-conformance reachable: the flagged dispatch would
 * not compile as ISO C17. Forwarding the list needs no extension, because
 * every caller passes at least the format string -- and a caller passing
 * *nothing* is now a diagnostic rather than silently accepted, which is the
 * right answer for a print with no format.
 *
 * `oz_platform.h` records the same C23-vs-C17 constraint for `OZM`, which
 * is why that one is not variadic-with-no-argument either.
 */
#define OZ_PLATFORM_PRINT(...) printf(__VA_ARGS__)
#define OZ_PLATFORM_SNPRINT(...) snprintf(__VA_ARGS__)

/*
 * Push buffered output out before something that will not return.
 *
 * `printf` to anything but a terminal is fully buffered, and `abort()` does
 * not flush stdio -- so a diagnostic printed immediately before an assert
 * is discarded precisely when it matters. #452's refcount traps print the
 * class name and then abort, and on glibc the class line and everything
 * before it vanished while the assertion text (stderr, unbuffered)
 * survived. It looked like the trap had failed to name the class; it had
 * named it into a buffer nobody flushed.
 *
 * `NULL` rather than `stdout`: every stream, because the caller is about to
 * stop the program and anything still held is lost.
 */
static inline void oz_platform_flush(void)
{
        (void)fflush(NULL);
}

/* ------------------------------------------------------------------ */
/* Heap allocator — malloc-backed wrapper for dynamicAllocWithHeap:           */
/* ------------------------------------------------------------------ */

#ifdef OZ_HEAP_SUPPORT
#define OZ_HEAP_INNER_DEFINED

/**
 * @brief Platform-specific heap inner type (Host).
 *
 * On the host backend, all heap paths use malloc — the inner struct
 * only exists for API compatibility with the Zephyr backend.
 */
struct oz_heap_inner {
        void *buf;
        size_t size;
        size_t allocated;
};

struct OZHeap;

struct oz_heap_hdr {
        struct OZHeap *heap;
        size_t alloc_size;
        char obj[];
};

static inline void oz_heap_init(struct oz_heap_inner *inner,
                                void *buf, size_t size)
{
        inner->buf = buf;
        inner->size = size;
        inner->allocated = 0;
}

static inline void *oz_heap_alloc_obj(struct oz_heap_inner *inner,
                                      struct OZHeap *owner, size_t size)
{
        size_t total = sizeof(struct oz_heap_hdr) + size;
        void *raw = malloc(total);
        if (!raw) {
                return NULL;
        }
        inner->allocated += total;
        struct oz_heap_hdr *hdr = (struct oz_heap_hdr *)raw;
        hdr->heap = owner;
        hdr->alloc_size = total;
        return hdr->obj;
}

static inline void oz_heap_free_obj(struct oz_heap_inner *inner, void *obj)
{
        struct oz_heap_hdr *hdr = (struct oz_heap_hdr *)
                ((char *)obj - offsetof(struct oz_heap_hdr, obj));
        if (inner->allocated >= hdr->alloc_size) {
                inner->allocated -= hdr->alloc_size;
        }
        free(hdr);
}

static inline size_t oz_heap_used_bytes(struct oz_heap_inner *inner)
{
        return inner->allocated;
}

static inline void *oz_sys_heap_alloc(size_t size)
{
        size_t total = sizeof(struct oz_heap_hdr) + size;
        void *raw = malloc(total);
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
        free(hdr);
}

/**
 * @brief Allocate from an OZHeap or system heap.
 * @brief Free a heap-allocated object (resolves heap via CONTAINER_OF).
 *
 * Defined in the generated oz_dispatch.c — requires struct OZHeap
 * to be complete.
 *
 * `oz_heap_`, like every other C-side name. These two are *declared* here
 * and *defined* by the companion, and #417 gave that split a prefix of its
 * own for a while; #462 retired it, because `oz2c` names the tool and not
 * the code it emits. One prefix per layer: `oz_`/`OZ_` for C, `_oz_` for
 * per-class internals.
 *
 * The word order is what #417 was really fixing, and it still holds:
 * subsystem, verb, then qualifier. So `oz_heap_alloc` calls
 * `oz_heap_alloc_obj` — a prefix pair, and deliberately not the anagram
 * `oz_heap_obj_alloc` calling `oz_heap_alloc_obj` that both preceded it.
 */
void *oz_heap_alloc(struct OZHeap *heap, size_t size);
void oz_heap_free(void *obj);

#endif /* OZ_HEAP_SUPPORT */

/* ------------------------------------------------------------------ */
/* Auto-initialization — constructor attribute for +initialize methods */
/* ------------------------------------------------------------------ */

#define OZ_AUTO_INIT(fn_name, init_fn)                                           \
        __attribute__((constructor))                                              \
        static void fn_name(void) { init_fn(); }

#endif /* OZ_PLATFORM_HOST_H */
