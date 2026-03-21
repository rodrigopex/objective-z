/**
 * @file OZHeap.h
 * @brief Heap manager for OZ objects — allocWithHeap: support.
 *
 * OZHeap wraps a sys_heap (Zephyr) or malloc pool (host) with
 * thread-safe locking.  Declare an OZHeap via slab, initialise
 * with -initWithBuffer:size:, then pass to [Cls allocWithHeap:].
 */

#pragma once

#import "OZObject.h"

/**
 * @brief Opaque heap inner type — stub for Clang AST analysis.
 *
 * The real definition lives in the PAL headers and is resolved
 * at GCC compile time.  The transpiler only needs the type name.
 */
#ifndef OZ_HEAP_INNER_DEFINED
struct oz_heap_inner {
	int _opaque;
};

/** @brief Stub for Clang AST analysis — real definition in PAL. */
static inline void oz_heap_init(struct oz_heap_inner *inner,
                                void *buf, size_t size)
{
	(void)inner;
	(void)buf;
	(void)size;
}

/** @brief Stub for Clang AST analysis — real definition in PAL. */
static inline size_t oz_heap_used_bytes(struct oz_heap_inner *inner)
{
	(void)inner;
	return 0;
}
#endif

@interface OZHeap : OZObject {
	struct oz_heap_inner _inner;
}
- (id)initWithBuffer:(void *)buf size:(int)size;
- (size_t)usedBytes;
@end
