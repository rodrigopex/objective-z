/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for memory (malloc.c), Object.m, and NXConstantString.m.
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>
#include <objc/malloc.h>
#include <string.h>

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_mem_create_item(int data);
extern int test_mem_item_data(id obj);
extern void test_mem_dealloc(id obj);
extern id test_mem_create_object(void);
extern BOOL test_mem_is_equal(id a, id b);
extern Class test_mem_get_class(id obj);
extern Class test_mem_get_superclass(id obj);
extern BOOL test_mem_responds_to_init(id obj);
extern BOOL test_mem_responds_to_nonexistent(id obj);
extern const char *test_mem_cstr_get(void);
extern unsigned int test_mem_cstr_length(void);
extern BOOL test_mem_cstr_equal_same(void);
extern BOOL test_mem_cstr_equal_diff(void);
extern BOOL test_mem_cstr_identity(void);

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(memory, NULL, NULL, NULL, NULL, NULL);

/* ── malloc.c tests ──────────────────────────────────────────────── */

/* Object alloc succeeds and dealloc does not crash */
ZTEST(memory, test_alloc_dealloc)
{
	id obj = test_mem_create_object();

	zassert_not_null(obj, "Object alloc should succeed");
	test_mem_dealloc(obj);
}

/* Allocated ivars are zeroed */
ZTEST(memory, test_alloc_zeroed)
{
	id item = test_mem_create_item(0);

	zassert_not_null(item, "alloc should succeed");
	zassert_equal(test_mem_item_data(item), 0,
		      "ivar should be 0 after alloc+init(0)");

	test_mem_dealloc(item);
}

/* Multiple alloc/dealloc cycles do not exhaust memory */
ZTEST(memory, test_multiple_alloc_dealloc)
{
	for (int i = 0; i < 50; i++) {
		id obj = test_mem_create_item(i);

		zassert_not_null(obj, "alloc should succeed on iteration");
		zassert_equal(test_mem_item_data(obj), i,
			      "data should match");
		test_mem_dealloc(obj);
	}
}

/* objc_stats returns valid data with CONFIG_SYS_HEAP_RUNTIME_STATS */
ZTEST(memory, test_heap_stats)
{
	struct sys_memory_stats stats;

	memset(&stats, 0, sizeof(stats));
	objc_stats(&stats);

	/* free_bytes should be > 0 since the heap is initialized */
	zassert_true(stats.free_bytes > 0,
		     "free_bytes should be positive");
}

/* Allocation followed by stats shows allocated bytes increased */
ZTEST(memory, test_heap_stats_after_alloc)
{
	struct sys_memory_stats before, after;

	objc_stats(&before);

	id obj = test_mem_create_item(42);

	zassert_not_null(obj, "alloc should succeed");
	objc_stats(&after);

	zassert_true(after.allocated_bytes > before.allocated_bytes,
		     "allocated_bytes should increase after alloc");

	test_mem_dealloc(obj);
}

/* ── Object.m tests ──────────────────────────────────────────────── */

/* [a isEqual:a] returns YES (identity) */
ZTEST(memory, test_object_isEqual_identity)
{
	id obj = test_mem_create_object();

	zassert_true(test_mem_is_equal(obj, obj),
		     "[obj isEqual:obj] should be YES");

	test_mem_dealloc(obj);
}

/* [a isEqual:b] returns NO for different objects */
ZTEST(memory, test_object_isEqual_different)
{
	id a = test_mem_create_object();
	id b = test_mem_create_object();

	zassert_false(test_mem_is_equal(a, b),
		      "[a isEqual:b] should be NO for different objects");

	test_mem_dealloc(a);
	test_mem_dealloc(b);
}

/* [obj class] returns the correct class */
ZTEST(memory, test_object_class)
{
	id obj = test_mem_create_item(0);
	Class cls = test_mem_get_class(obj);
	Class expected = objc_lookupClass("TestItem");

	zassert_equal(cls, expected,
		      "[obj class] should return TestItem");

	test_mem_dealloc(obj);
}

/* [rootObj superclass] returns Nil for root class */
ZTEST(memory, test_object_superclass_root)
{
	id obj = test_mem_create_object();
	Class super = test_mem_get_superclass(obj);

	zassert_is_null(super,
			"Object superclass should be Nil");

	test_mem_dealloc(obj);
}

/* [obj respondsToSelector] YES for known, NO for unknown */
ZTEST(memory, test_object_responds_to_selector)
{
	id obj = test_mem_create_object();

	zassert_true(test_mem_responds_to_init(obj),
		     "Object should respond to init");
	zassert_false(test_mem_responds_to_nonexistent(obj),
		      "Object should NOT respond to unknown method");

	test_mem_dealloc(obj);
}

/* ── NXConstantString tests ──────────────────────────────────────── */

/* @"hello" has correct cStr and length */
ZTEST(memory, test_constant_string_cstr)
{
	const char *s = test_mem_cstr_get();

	zassert_not_null(s, "cStr should not be NULL");
	zassert_equal(strcmp(s, "hello"), 0,
		      "cStr should be 'hello'");
}

ZTEST(memory, test_constant_string_length)
{
	zassert_equal(test_mem_cstr_length(), 5,
		      "length of 'hello' should be 5");
}

/* @"hello" isEqual:@"hello" returns YES */
ZTEST(memory, test_constant_string_equal_same)
{
	zassert_true(test_mem_cstr_equal_same(),
		     "@\"hello\" isEqual:@\"hello\" should be YES");
}

/* @"hello" isEqual:@"world" returns NO */
ZTEST(memory, test_constant_string_equal_diff)
{
	zassert_false(test_mem_cstr_equal_diff(),
		      "@\"hello\" isEqual:@\"world\" should be NO");
}

/* Same literal identity check */
ZTEST(memory, test_constant_string_identity)
{
	zassert_true(test_mem_cstr_identity(),
		     "same literal isEqual:self should be YES");
}
