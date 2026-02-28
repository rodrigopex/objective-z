/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helpers for blocks tests.
 *
 * Compiled with Clang + -fblocks (via objz_target_sources).
 * Provides C-callable wrappers for block operations.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>
#import <objc/blocks.h>
#include <objc/arc.h>
#include <objc/runtime.h>

/* ── Dealloc tracking ──────────────────────────────────────────── */

int g_blocks_dealloc_count = 0;

@interface BlockTestObj : Object {
	int _tag;
}
- (id)initWithTag:(int)tag;
- (int)tag;
@end

@implementation BlockTestObj

- (id)initWithTag:(int)tag
{
	self = [super init];
	if (self) {
		_tag = tag;
	}
	return self;
}

- (int)tag
{
	return _tag;
}

- (void)dealloc
{
	g_blocks_dealloc_count++;
}

@end

/* ── Test: global block (no captures) ──────────────────────────── */

typedef int (^IntBlock)(void);

static int global_block_result(void)
{
	return 42;
}

/*
 * A block with no captures is emitted as a global block by Clang.
 * _Block_copy on a global block returns the same pointer.
 */
void *test_blocks_global_block(void)
{
	IntBlock blk = ^{ return 42; };
	(void)global_block_result;
	return (__bridge void *)blk;
}

void *test_blocks_copy_global(void *blk)
{
	return _Block_copy((const void *)blk);
}

void test_blocks_release(void *blk)
{
	_Block_release(blk);
}

int test_blocks_invoke_int_block(void *blk)
{
	return ((__bridge IntBlock)blk)();
}

/* ── Test: stack block with captures ───────────────────────────── */

/*
 * Returns a heap-copied block that captures an int.
 * The caller owns the returned block.
 */
void *test_blocks_copy_capturing_block(int value)
{
	IntBlock blk = ^{
		return value;
	};
	return _Block_copy((__bridge const void *)blk);
}

/* ── Test: block capturing ObjC object ─────────────────────────── */

typedef int (^ObjBlock)(void);

void *test_blocks_create_obj(int tag)
{
	return (__bridge_retained void *)[[BlockTestObj alloc] initWithTag:tag];
}

unsigned int test_blocks_get_rc(__unsafe_unretained id obj)
{
	return __objc_refcount_get(obj);
}

void test_blocks_release_obj(__unsafe_unretained id obj)
{
	objc_release(obj);
}

void test_blocks_reset_dealloc_count(void)
{
	g_blocks_dealloc_count = 0;
}

/*
 * Create and copy a block that captures an ObjC object.
 * The block's copy helper should retain the captured object.
 */
void *test_blocks_copy_obj_capturing_block(id obj)
{
	ObjBlock blk = ^{
		return [obj tag];
	};
	return _Block_copy((__bridge const void *)blk);
}

/* ── Test: __block variable ────────────────────────────────────── */

typedef void (^VoidBlock)(void);

/*
 * Create a block that mutates a __block variable.
 * Returns the heap-copied block; *out_value receives the
 * initial value of the __block variable.
 */
void *test_blocks_byref_block(int *out_value)
{
	__block int counter = 0;
	VoidBlock blk = ^{
		counter++;
	};
	void *copied = _Block_copy((__bridge const void *)blk);
	*out_value = counter;
	return copied;
}

/*
 * Invoke a VoidBlock and return the __block variable value
 * via the forwarding pointer.
 */
void test_blocks_invoke_void_block(void *blk)
{
	((__bridge VoidBlock)blk)();
}

/* ── Test: nested blocks (block capturing block) ───────────────── */

void *test_blocks_nested(int value)
{
	IntBlock inner = ^{
		return value;
	};
	IntBlock outer = ^{
		return inner();
	};
	return _Block_copy((__bridge const void *)outer);
}
