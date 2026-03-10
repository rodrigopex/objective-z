/**
 * @file blocks.c
 * @brief Blocks (closures) runtime implementation.
 *
 * Implements the LLVM Block ABI entry points: _Block_copy,
 * _Block_release, _Block_object_assign, _Block_object_dispose.
 * Refcount is stored in Block_layout.flags bits 1-15.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#include "api.h"
#include <objc/blocks.h>
#include <objc/malloc.h>
#include <objc/runtime.h>
#include <string.h>
#include <zephyr/sys/atomic.h>

/* message.c */
extern IMP objc_msg_lookup(id receiver, SEL selector);

/* ── Block isa class pointers ──────────────────────────────────── */

/*
 * Clang uses these as isa tag values. They do not need to be real
 * class structures — just unique, non-NULL addresses.
 */
void *_NSConcreteStackBlock[1];
void *_NSConcreteGlobalBlock[1];
void *_NSConcreteMallocBlock[1];

/* ── Selectors for retain/release dispatch ─────────────────────── */

static struct objc_selector retain_sel = { .name = "retain", .types = NULL };
static struct objc_selector release_sel = { .name = "release", .types = NULL };

static inline void __block_retain_object(id obj)
{
	if (obj == nil) {
		return;
	}
	IMP imp = objc_msg_lookup(obj, &retain_sel);
	if (imp != NULL) {
		((id (*)(id, SEL))imp)(obj, &retain_sel);
	}
}

static inline void __block_release_object(id obj)
{
	if (obj == nil) {
		return;
	}
	IMP imp = objc_msg_lookup(obj, &release_sel);
	if (imp != NULL) {
		((void (*)(id, SEL))imp)(obj, &release_sel);
	}
}

/* ── Helpers ───────────────────────────────────────────────────── */

static inline struct Block_descriptor_2 *
__block_descriptor_2(struct Block_layout *block)
{
	uint8_t *desc = (uint8_t *)block->descriptor;
	desc += sizeof(struct Block_descriptor_1);
	return (struct Block_descriptor_2 *)desc;
}

static inline struct Block_byref_2 *
__block_byref_2(struct Block_byref *byref)
{
	uint8_t *p = (uint8_t *)byref;
	p += sizeof(struct Block_byref);
	return (struct Block_byref_2 *)p;
}

/* ── Byref copy/release ────────────────────────────────────────── */

static struct Block_byref *__block_byref_copy(struct Block_byref *src)
{
	struct Block_byref *copy;

	/* Already on the heap — just bump refcount */
	if (src->forwarding != src) {
		copy = src->forwarding;
		copy->flags += 2; /* increment refcount (bit 1 step) */
		return copy;
	}

	copy = (struct Block_byref *)objc_malloc(src->size);
	if (copy == NULL) {
		return NULL;
	}
	memcpy(copy, src, src->size);

	/* Point both forwarding pointers to the heap copy */
	copy->forwarding = copy;
	src->forwarding = copy;

	/*
	 * Mark as heap-allocated, refcount = 2 (encoded as 4 in bits 1-15).
	 * Two references: one held by the heap block, one by the stack scope
	 * (Clang emits _Block_object_dispose at function exit for __block vars).
	 */
	copy->flags = (src->flags & ~BLOCK_BYREF_REFCOUNT_MASK) | BLOCK_BYREF_NEEDS_FREE | 4;

	/* Call byref copy helper if present */
	if (src->flags & BLOCK_BYREF_HAS_COPY_DISPOSE) {
		struct Block_byref_2 *helpers = __block_byref_2(copy);
		helpers->byref_keep(copy, src);
	}

	return copy;
}

static void __block_byref_release(struct Block_byref *byref)
{
	struct Block_byref *shared;

	if (byref == NULL) {
		return;
	}

	shared = byref->forwarding;

	/* Not a heap byref — nothing to do */
	if (!(shared->flags & BLOCK_BYREF_NEEDS_FREE)) {
		return;
	}

	/* Decrement refcount (bits 1-15) */
	int old_flags = shared->flags;
	int old_rc = old_flags & BLOCK_BYREF_REFCOUNT_MASK;

	if (old_rc > 2) {
		shared->flags = old_flags - 2;
		return;
	}

	/* Refcount reached zero — dispose and free */
	if (shared->flags & BLOCK_BYREF_HAS_COPY_DISPOSE) {
		struct Block_byref_2 *helpers = __block_byref_2(shared);
		helpers->byref_destroy(shared);
	}

	objc_free(shared);
}

/* ── _Block_copy ───────────────────────────────────────────────── */

void *_Block_copy(const void *arg)
{
	struct Block_layout *src;
	struct Block_layout *copy;

	if (arg == NULL) {
		return NULL;
	}

	src = (struct Block_layout *)arg;

	/* Global blocks are immortal — return as-is */
	if (src->flags & BLOCK_IS_GLOBAL) {
		return (void *)src;
	}

	/* Already a malloc block — bump refcount */
	if (src->flags & BLOCK_NEEDS_FREE) {
		/*
		 * Refcount lives in bits 1-15. Adding 2 increments the
		 * refcount field by 1 (since bit 0 is BLOCK_DEALLOCATING).
		 */
		atomic_val_t *flags_ptr = (atomic_val_t *)&src->flags;
		atomic_add(flags_ptr, 2);
		return (void *)src;
	}

	/* Stack block — copy to heap */
	unsigned long size = src->descriptor->size;
	copy = (struct Block_layout *)objc_malloc(size);
	if (copy == NULL) {
		return NULL;
	}

	memcpy(copy, src, size);

	/* Set isa to malloc block and mark as heap-allocated */
	copy->isa = _NSConcreteMallocBlock;
	copy->flags = (src->flags & ~BLOCK_REFCOUNT_MASK) | BLOCK_NEEDS_FREE | 2;

	/* Call the copy helper to handle captured variables */
	if (src->flags & BLOCK_HAS_COPY_DISPOSE) {
		struct Block_descriptor_2 *desc2 = __block_descriptor_2(copy);
		desc2->copy(copy, src);
	}

	return (void *)copy;
}

/* ── _Block_release ────────────────────────────────────────────── */

void _Block_release(const void *arg)
{
	struct Block_layout *block;

	if (arg == NULL) {
		return;
	}

	block = (struct Block_layout *)arg;

	/* Global or stack blocks — nothing to do */
	if (block->flags & BLOCK_IS_GLOBAL) {
		return;
	}
	if (!(block->flags & BLOCK_NEEDS_FREE)) {
		return;
	}

	/* Decrement refcount atomically */
	atomic_val_t *flags_ptr = (atomic_val_t *)&block->flags;
	atomic_val_t old = atomic_sub(flags_ptr, 2);
	int old_rc = old & BLOCK_REFCOUNT_MASK;

	/* refcount was 1 (encoded as 2) — time to destroy */
	if (old_rc == 2) {
		if (old & BLOCK_HAS_COPY_DISPOSE) {
			struct Block_descriptor_2 *desc2 = __block_descriptor_2(block);
			desc2->dispose(block);
		}
		objc_free(block);
	}
}

/* ── _Block_object_assign ──────────────────────────────────────── */

void _Block_object_assign(void *destArg, const void *src, const int flags)
{
	void **dest = (void **)destArg;

	switch (flags & 0x1f) {
	case BLOCK_FIELD_IS_OBJECT:
		/*
		 * Retain the captured ObjC object via message dispatch.
		 * Works in both MRR and ARC modes.
		 */
		__block_retain_object((id)src);
		*dest = (void *)src;
		break;
	case BLOCK_FIELD_IS_BLOCK:
		/* Captured block — copy it */
		*dest = _Block_copy(src);
		break;
	case BLOCK_FIELD_IS_BYREF:
		/* __block variable — copy the byref struct */
		*dest = __block_byref_copy((struct Block_byref *)src);
		break;
	default:
		/* Unknown field type — just copy the pointer */
		*dest = (void *)src;
		break;
	}
}

/* ── _Block_object_dispose ─────────────────────────────────────── */

void _Block_object_dispose(const void *object, const int flags)
{
	if (object == NULL) {
		return;
	}

	switch (flags & 0x1f) {
	case BLOCK_FIELD_IS_OBJECT:
		__block_release_object((id)object);
		break;
	case BLOCK_FIELD_IS_BLOCK:
		_Block_release(object);
		break;
	case BLOCK_FIELD_IS_BYREF:
		__block_byref_release((struct Block_byref *)object);
		break;
	default:
		break;
	}
}
