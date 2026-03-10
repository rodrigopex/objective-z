/**
 * @file blocks.h
 * @brief Blocks (closures) runtime support.
 *
 * Provides the ABI structures and entry points required by Clang
 * when compiling with -fblocks.  Follows the LLVM Block Implementation
 * Specification.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

/**
 * @defgroup blocks Blocks Runtime
 * @ingroup objc
 * @{
 */

/* ── Block layout flags (stored in Block_layout.flags) ─────────── */

enum {
	BLOCK_DEALLOCATING = 0x0001,
	BLOCK_REFCOUNT_MASK = 0xfffe,     /* bits 1..15: refcount - 1 */
	BLOCK_IS_NOESCAPE = (1 << 23),
	BLOCK_NEEDS_FREE = (1 << 24),     /* heap (malloc) block */
	BLOCK_HAS_COPY_DISPOSE = (1 << 25),
	BLOCK_IS_GLOBAL = (1 << 28),
	BLOCK_HAS_SIGNATURE = (1 << 30),
};

/* ── Block_object_assign / dispose flags ───────────────────────── */

enum {
	BLOCK_FIELD_IS_OBJECT = 3,  /* captured ObjC object */
	BLOCK_FIELD_IS_BLOCK = 7,   /* captured block */
	BLOCK_FIELD_IS_BYREF = 8,   /* __block variable */
	BLOCK_FIELD_IS_WEAK = 16,   /* __weak captured object */
	BLOCK_BYREF_CALLER = 128,   /* internal: from byref copy/dispose */
};

/* ── Byref flags ───────────────────────────────────────────────── */

enum {
	BLOCK_BYREF_HAS_COPY_DISPOSE = (1 << 25),
	BLOCK_BYREF_NEEDS_FREE = (1 << 24),
	BLOCK_BYREF_REFCOUNT_MASK = 0xfffe,
};

/* ── Block descriptor (always present after invoke pointer) ────── */

struct Block_descriptor_1 {
	unsigned long reserved;
	unsigned long size; /* sizeof(Block_layout + captured variables) */
};

/* Optional: present when BLOCK_HAS_COPY_DISPOSE is set */
struct Block_descriptor_2 {
	void (*copy)(void *dst, const void *src);
	void (*dispose)(const void *);
};

/* Optional: present when BLOCK_HAS_SIGNATURE is set */
struct Block_descriptor_3 {
	const char *encoding;
	const char *layout;
};

/* ── Block_layout: the in-memory representation of a block ─────── */

struct Block_layout {
	void *isa;
	volatile int flags;
	int reserved;
	void (*invoke)(void *, ...);
	struct Block_descriptor_1 *descriptor;
	/* captured variables follow */
};

/* ── __block variable support ──────────────────────────────────── */

struct Block_byref {
	void *isa;
	struct Block_byref *forwarding;
	volatile int flags;
	unsigned int size;
	/* Optional copy/dispose helpers follow if BLOCK_BYREF_HAS_COPY_DISPOSE */
	/* Actual variable data follows */
};

struct Block_byref_2 {
	void (*byref_keep)(struct Block_byref *dst, struct Block_byref *src);
	void (*byref_destroy)(struct Block_byref *);
};

/* ── Block isa class pointers (Clang references these) ─────────── */

extern void *_NSConcreteStackBlock[];
extern void *_NSConcreteGlobalBlock[];
extern void *_NSConcreteMallocBlock[];

/* ── Public API ────────────────────────────────────────────────── */

/** Copy a block: stack → malloc, or retain an existing malloc block. */
void *_Block_copy(const void *block);

/** Release a malloc block. No-op for global/stack blocks. */
void _Block_release(const void *block);

/**
 * Called by block copy helpers to handle captured variables.
 * @param dest  Pointer to destination slot in the new (malloc) block.
 * @param src   The captured value to assign.
 * @param flags BLOCK_FIELD_IS_OBJECT, BLOCK_FIELD_IS_BLOCK, etc.
 */
void _Block_object_assign(void *dest, const void *src, const int flags);

/**
 * Called by block dispose helpers to release captured variables.
 * @param object The captured value to dispose.
 * @param flags  BLOCK_FIELD_IS_OBJECT, BLOCK_FIELD_IS_BLOCK, etc.
 */
void _Block_object_dispose(const void *object, const int flags);

/** @} */
