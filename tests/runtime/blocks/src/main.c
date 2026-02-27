/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for blocks (closures) runtime.
 *
 * Exercises _Block_copy, _Block_release, object capture lifecycle,
 * __block variable forwarding, and nested blocks.
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>
#include <objc/blocks.h>

/* ── Helpers (defined in helpers.m) ────────────────────────────── */

extern void *test_blocks_global_block(void);
extern void *test_blocks_copy_global(void *blk);
extern void test_blocks_release(void *blk);
extern int test_blocks_invoke_int_block(void *blk);

extern void *test_blocks_copy_capturing_block(int value);

extern void *test_blocks_create_obj(int tag);
extern unsigned int test_blocks_get_rc(id obj);
extern void test_blocks_release_obj(id obj);
extern void test_blocks_reset_dealloc_count(void);
extern void *test_blocks_copy_obj_capturing_block(id obj);

extern void *test_blocks_byref_block(int *out_value);
extern void test_blocks_invoke_void_block(void *blk);

extern void *test_blocks_nested(int value);

extern int g_blocks_dealloc_count;

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(blocks, NULL, NULL, NULL, NULL, NULL);

/* Global block: _Block_copy returns the same pointer */
ZTEST(blocks, test_global_block_copy_is_identity)
{
	void *blk = test_blocks_global_block();

	zassert_not_null(blk, "global block should not be NULL");

	void *copy = test_blocks_copy_global(blk);

	zassert_equal(copy, blk,
		      "_Block_copy of global block should return same pointer");

	/* Release should be a no-op for global blocks */
	test_blocks_release(copy);
}

/* Global block: invocation works */
ZTEST(blocks, test_global_block_invocation)
{
	void *blk = test_blocks_global_block();
	int result = test_blocks_invoke_int_block(blk);

	zassert_equal(result, 42, "global block should return 42");
}

/* Stack block with int capture: copy promotes to heap */
ZTEST(blocks, test_stack_block_copy)
{
	void *blk = test_blocks_copy_capturing_block(99);

	zassert_not_null(blk, "copied block should not be NULL");

	int result = test_blocks_invoke_int_block(blk);

	zassert_equal(result, 99,
		      "copied block should capture value 99");

	test_blocks_release(blk);
}

/* Copied block can be retained and released */
ZTEST(blocks, test_block_retain_release)
{
	void *blk = test_blocks_copy_capturing_block(7);

	zassert_not_null(blk, "copied block should not be NULL");

	/* Second copy increments refcount */
	void *blk2 = test_blocks_copy_global(blk);

	zassert_equal(blk2, blk,
		      "copy of malloc block should return same pointer");

	/* First release — block should still be alive */
	test_blocks_release(blk);
	int result = test_blocks_invoke_int_block(blk2);

	zassert_equal(result, 7,
		      "block should still work after one release");

	/* Second release — block is freed */
	test_blocks_release(blk2);
}

/* Block capturing ObjC object: copy retains, release drops */
ZTEST(blocks, test_block_captures_object)
{
	test_blocks_reset_dealloc_count();

	id obj = test_blocks_create_obj(55);

	zassert_not_null(obj, "alloc should succeed");
	zassert_equal(test_blocks_get_rc(obj), 1, "initial rc should be 1");

	/* Copy block — should retain captured object */
	void *blk = test_blocks_copy_obj_capturing_block(obj);

	zassert_not_null(blk, "block copy should succeed");
	zassert_equal(test_blocks_get_rc(obj), 2,
		      "block copy should retain captured object");

	/* Invoke block — should access captured object */
	int result = test_blocks_invoke_int_block(blk);

	zassert_equal(result, 55,
		      "block should read captured object's tag");

	/* Release block — should release captured object */
	test_blocks_release(blk);
	zassert_equal(test_blocks_get_rc(obj), 1,
		      "block release should release captured object");

	/* Final release of object */
	test_blocks_release_obj(obj);
	zassert_equal(g_blocks_dealloc_count, 1,
		      "object should be deallocated");
}

/* __block variable: mutation via forwarding pointer */
ZTEST(blocks, test_byref_variable)
{
	int initial = -1;
	void *blk = test_blocks_byref_block(&initial);

	zassert_not_null(blk, "byref block copy should succeed");
	zassert_equal(initial, 0, "__block variable should start at 0");

	/* Invoke block — should increment the __block variable */
	test_blocks_invoke_void_block(blk);
	test_blocks_invoke_void_block(blk);
	test_blocks_invoke_void_block(blk);

	test_blocks_release(blk);
}

/* Nested block: outer captures inner */
ZTEST(blocks, test_nested_blocks)
{
	void *blk = test_blocks_nested(77);

	zassert_not_null(blk, "nested block copy should succeed");

	int result = test_blocks_invoke_int_block(blk);

	zassert_equal(result, 77,
		      "nested block should return captured value");

	test_blocks_release(blk);
}

/* _Block_copy(NULL) returns NULL */
ZTEST(blocks, test_copy_null)
{
	void *result = _Block_copy(NULL);

	zassert_is_null(result, "_Block_copy(NULL) should return NULL");
}

/* _Block_release(NULL) does not crash */
ZTEST(blocks, test_release_null)
{
	_Block_release(NULL);
	zassert_true(true, "_Block_release(NULL) should not crash");
}
