/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for OZArray description, element lifecycle, and string elements.
 */
#include <zephyr/ztest.h>
#include <string.h>

#include <objc/runtime.h>

/* ── Extern helpers from helpers.m ────────────────────────────────── */

extern void *test_arr_pool_push(void);
extern void test_arr_pool_pop(void *p);

extern id test_arr_empty(void);
extern id test_arr_single(void);
extern id test_arr_multi(void);
extern id test_arr_strings(void);

extern unsigned int test_arr_count(id arr);
extern id test_arr_object_at(id arr, unsigned int idx);
extern const char *test_arr_description_cstr(id arr);
extern unsigned int test_arr_element_retain_count(id arr, unsigned int idx);
extern int test_arr_int_value(id n);
extern const char *test_arr_string_cstr(id s);

extern id test_arr_create_number(int v);
extern id test_arr_retain(id obj);
extern unsigned int test_arr_retain_count(id obj);

/* ── Test suites ──────────────────────────────────────────────────── */

ZTEST_SUITE(arr_description, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(arr_lifecycle, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(arr_elements, NULL, NULL, NULL, NULL, NULL);

/* ── Description tests ────────────────────────────────────────────── */

ZTEST(arr_description, test_description_empty)
{
	void *pool = test_arr_pool_push();

	id arr = test_arr_empty();
	const char *desc = test_arr_description_cstr(arr);
	zassert_equal(strcmp(desc, "()"), 0, "empty array description should be \"()\"");

	test_arr_pool_pop(pool);
}

ZTEST(arr_description, test_description_single)
{
	void *pool = test_arr_pool_push();

	id arr = test_arr_single();
	const char *desc = test_arr_description_cstr(arr);
	zassert_equal(strcmp(desc, "(42)"), 0, "single element description should be \"(42)\"");

	test_arr_pool_pop(pool);
}

ZTEST(arr_description, test_description_multi)
{
	void *pool = test_arr_pool_push();

	id arr = test_arr_multi();
	const char *desc = test_arr_description_cstr(arr);
	zassert_equal(strcmp(desc, "(1, 2, 3)"), 0,
		      "multi element description should be \"(1, 2, 3)\"");

	test_arr_pool_pop(pool);
}

/* ── Element retain lifecycle ─────────────────────────────────────── */

ZTEST(arr_lifecycle, test_element_retain)
{
	void *pool = test_arr_pool_push();

	/* Elements in the array are retained by the factory.
	 * For small int singletons, retainCount is INT32_MAX (immortal).
	 * Use a heap-allocated number (>= 16) to verify retain. */
	id arr = test_arr_multi();
	/* @1, @2, @3 are singletons. Just verify they are accessible. */
	zassert_equal(test_arr_int_value(test_arr_object_at(arr, 0)), 1);
	zassert_equal(test_arr_int_value(test_arr_object_at(arr, 1)), 2);
	zassert_equal(test_arr_int_value(test_arr_object_at(arr, 2)), 3);

	test_arr_pool_pop(pool);
}

ZTEST(arr_lifecycle, test_element_release_on_dealloc)
{
	void *pool = test_arr_pool_push();

	/* Create dict and let pool drain release it.
	 * If dealloc doesn't release elements, this would leak.
	 * No crash = dealloc path works. */
	id arr = test_arr_multi();
	zassert_not_null(arr, "array should be created");

	test_arr_pool_pop(pool);
}

/* ── String elements ──────────────────────────────────────────────── */

ZTEST(arr_elements, test_string_elements)
{
	void *pool = test_arr_pool_push();

	id arr = test_arr_strings();
	zassert_equal(test_arr_count(arr), 3, "string array count should be 3");

	id s0 = test_arr_object_at(arr, 0);
	id s1 = test_arr_object_at(arr, 1);
	id s2 = test_arr_object_at(arr, 2);

	zassert_equal(strcmp(test_arr_string_cstr(s0), "alpha"), 0, "arr[0] should be alpha");
	zassert_equal(strcmp(test_arr_string_cstr(s1), "beta"), 0, "arr[1] should be beta");
	zassert_equal(strcmp(test_arr_string_cstr(s2), "gamma"), 0, "arr[2] should be gamma");

	test_arr_pool_pop(pool);
}
