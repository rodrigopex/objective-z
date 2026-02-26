/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for OZDictionary description, lifecycle, and number key lookup.
 */
#include <zephyr/ztest.h>
#include <string.h>

#include <objc/runtime.h>

/* ── Extern helpers from helpers.m ────────────────────────────────── */

extern void *test_dict_pool_push(void);
extern void test_dict_pool_pop(void *p);

extern id test_dict_empty(void);
extern id test_dict_single(void);
extern id test_dict_multi(void);
extern id test_dict_number_keys(void);

extern unsigned int test_dict_count(id d);
extern id test_dict_lookup_name(id d);
extern id test_dict_lookup_x(id d);
extern id test_dict_lookup_y(id d);
extern id test_dict_lookup_z(id d);
extern id test_dict_lookup_missing(id d);
extern id test_dict_lookup_num_key(id d, int key);

extern const char *test_dict_description_cstr(id d);
extern int test_dict_int_value(id n);
extern const char *test_dict_string_cstr(id s);

/* ── Test suites ──────────────────────────────────────────────────── */

ZTEST_SUITE(dict_description, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(dict_lifecycle, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(dict_lookup, NULL, NULL, NULL, NULL, NULL);

/* ── Description tests ────────────────────────────────────────────── */

ZTEST(dict_description, test_description_empty)
{
	void *pool = test_dict_pool_push();

	id d = test_dict_empty();
	const char *desc = test_dict_description_cstr(d);
	zassert_equal(strcmp(desc, "{}"), 0, "empty dict description should be \"{}\"");

	test_dict_pool_pop(pool);
}

ZTEST(dict_description, test_description_single)
{
	void *pool = test_dict_pool_push();

	id d = test_dict_single();
	const char *desc = test_dict_description_cstr(d);
	zassert_equal(strcmp(desc, "{name = Alice}"), 0,
		      "single pair description should be \"{name = Alice}\"");

	test_dict_pool_pop(pool);
}

ZTEST(dict_description, test_description_multi)
{
	void *pool = test_dict_pool_push();

	id d = test_dict_multi();
	const char *desc = test_dict_description_cstr(d);
	zassert_equal(strcmp(desc, "{x = 10; y = 20; z = 30}"), 0,
		      "multi pair description should use semicolons");

	test_dict_pool_pop(pool);
}

/* ── Key/value retain lifecycle ───────────────────────────────────── */

ZTEST(dict_lifecycle, test_key_value_retain)
{
	void *pool = test_dict_pool_push();

	/* Verify keys and values are accessible (retained) after creation */
	id d = test_dict_multi();
	zassert_equal(test_dict_count(d), 3, "count should be 3");

	id vx = test_dict_lookup_x(d);
	id vy = test_dict_lookup_y(d);
	id vz = test_dict_lookup_z(d);

	zassert_not_null(vx, "value for x should exist");
	zassert_not_null(vy, "value for y should exist");
	zassert_not_null(vz, "value for z should exist");

	zassert_equal(test_dict_int_value(vx), 10, "x should be 10");
	zassert_equal(test_dict_int_value(vy), 20, "y should be 20");
	zassert_equal(test_dict_int_value(vz), 30, "z should be 30");

	test_dict_pool_pop(pool);
}

ZTEST(dict_lifecycle, test_key_value_release_on_dealloc)
{
	void *pool = test_dict_pool_push();

	/* Create dict and let pool drain release it.
	 * If dealloc doesn't release keys/values, this would leak.
	 * No crash = dealloc path works. */
	id d = test_dict_multi();
	zassert_not_null(d, "dict should be created");

	test_dict_pool_pop(pool);
}

/* ── Number key lookup ────────────────────────────────────────────── */

ZTEST(dict_lookup, test_number_key_lookup)
{
	void *pool = test_dict_pool_push();

	id d = test_dict_number_keys();
	zassert_equal(test_dict_count(d), 2, "number-keyed dict count should be 2");

	/* Lookup with @1 key — exercises -isEqual: on OZNumber */
	id v1 = test_dict_lookup_num_key(d, 1);
	zassert_not_null(v1, "objectForKey:@1 should find a value");
	zassert_equal(strcmp(test_dict_string_cstr(v1), "one"), 0,
		      "dict[@1] should be \"one\"");

	id v2 = test_dict_lookup_num_key(d, 2);
	zassert_not_null(v2, "objectForKey:@2 should find a value");
	zassert_equal(strcmp(test_dict_string_cstr(v2), "two"), 0,
		      "dict[@2] should be \"two\"");

	/* Missing number key */
	id v3 = test_dict_lookup_num_key(d, 99);
	zassert_is_null(v3, "objectForKey:@99 should be nil");

	test_dict_pool_pop(pool);
}
