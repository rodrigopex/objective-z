/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for OZNumber, OZArray, and OZDictionary literal classes.
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── Pool management (helpers.m) ───────────────────────────────── */

extern void *test_lit_pool_push(void);
extern void test_lit_pool_pop(void *p);

/* ── OZNumber helpers ──────────────────────────────────────────── */

extern id test_lit_bool_yes(void);
extern id test_lit_bool_no(void);
extern id test_lit_int(int v);
extern id test_lit_double(double v);

extern bool test_lit_number_bool_value(id n);
extern int test_lit_number_int_value(id n);
extern double test_lit_number_double_value(id n);
extern unsigned int test_lit_number_hash(id n);
extern bool test_lit_number_is_equal(id a, id b);
extern unsigned int test_lit_number_retain_count(id n);

/* ── OZArray helpers ───────────────────────────────────────────── */

extern id test_lit_array_empty(void);
extern id test_lit_array_two(void);
extern id test_lit_array_strings(void);
extern unsigned int test_lit_array_count(id arr);
extern id test_lit_array_object_at(id arr, unsigned int idx);
extern id test_lit_array_subscript(id arr, unsigned int idx);

/* ── OZDictionary helpers ──────────────────────────────────────── */

extern id test_lit_dict_empty(void);
extern id test_lit_dict_one(void);
extern id test_lit_dict_multi(void);
extern unsigned int test_lit_dict_count(id d);
extern id test_lit_dict_lookup_key(id d);
extern id test_lit_dict_lookup_a(id d);
extern id test_lit_dict_lookup_b(id d);
extern id test_lit_dict_lookup_missing(id d);
extern id test_lit_dict_subscript_key(id d);

/* ── Test suites ───────────────────────────────────────────────── */

ZTEST_SUITE(number, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(array, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(dictionary, NULL, NULL, NULL, NULL, NULL);

/* ── OZNumber tests ────────────────────────────────────────────── */

ZTEST(number, test_bool_yes_value)
{
	void *pool = test_lit_pool_push();

	id yes = test_lit_bool_yes();
	zassert_true(test_lit_number_bool_value(yes), "@YES boolValue should be true");
	zassert_equal(test_lit_number_int_value(yes), 1, "@YES intValue should be 1");

	test_lit_pool_pop(pool);
}

ZTEST(number, test_bool_no_value)
{
	void *pool = test_lit_pool_push();

	id no = test_lit_bool_no();
	zassert_false(test_lit_number_bool_value(no), "@NO boolValue should be false");
	zassert_equal(test_lit_number_int_value(no), 0, "@NO intValue should be 0");

	test_lit_pool_pop(pool);
}

ZTEST(number, test_bool_singleton)
{
	void *pool = test_lit_pool_push();

	id a = test_lit_bool_yes();
	id b = test_lit_bool_yes();
	zassert_equal(a, b, "@YES should always return same pointer");

	id c = test_lit_bool_no();
	id d = test_lit_bool_no();
	zassert_equal(c, d, "@NO should always return same pointer");

	zassert_not_equal(a, c, "@YES and @NO must be different pointers");

	test_lit_pool_pop(pool);
}

ZTEST(number, test_small_int_cache)
{
	void *pool = test_lit_pool_push();

	/* Values 0..15 should be cached singletons */
	id a = test_lit_int(0);
	id b = test_lit_int(0);
	zassert_equal(a, b, "@0 should return same pointer");

	id c = test_lit_int(15);
	id d = test_lit_int(15);
	zassert_equal(c, d, "@15 should return same pointer");

	zassert_equal(test_lit_number_int_value(a), 0);
	zassert_equal(test_lit_number_int_value(c), 15);

	test_lit_pool_pop(pool);
}

ZTEST(number, test_heap_int)
{
	void *pool = test_lit_pool_push();

	id n = test_lit_int(1000);
	zassert_equal(test_lit_number_int_value(n), 1000);

	/* Heap-allocated numbers should not be singletons */
	id m = test_lit_int(1000);
	zassert_not_equal(n, m, "Heap ints should be distinct objects");

	test_lit_pool_pop(pool);
}

ZTEST(number, test_double_value)
{
	void *pool = test_lit_pool_push();

	id n = test_lit_double(3.14);
	double val = test_lit_number_double_value(n);

	/* Floating point comparison with tolerance */
	zassert_true(val > 3.13 && val < 3.15, "@3.14 doubleValue should be ~3.14");
	zassert_equal(test_lit_number_int_value(n), 3, "@3.14 intValue should be 3");

	test_lit_pool_pop(pool);
}

ZTEST(number, test_is_equal)
{
	void *pool = test_lit_pool_push();

	id a = test_lit_int(42);
	id b = test_lit_int(42);
	zassert_true(test_lit_number_is_equal(a, b), "@42 isEqual: @42");

	id c = test_lit_int(99);
	zassert_false(test_lit_number_is_equal(a, c), "@42 !isEqual: @99");

	/* Bool and int comparison */
	id yes = test_lit_bool_yes();
	id one = test_lit_int(1);
	zassert_true(test_lit_number_is_equal(yes, one), "@YES isEqual: @1");

	test_lit_pool_pop(pool);
}

ZTEST(number, test_hash)
{
	void *pool = test_lit_pool_push();

	id a = test_lit_int(42);
	id b = test_lit_int(42);
	zassert_equal(test_lit_number_hash(a), test_lit_number_hash(b),
		      "Equal numbers should have equal hash");

	test_lit_pool_pop(pool);
}

/* ── OZArray tests ─────────────────────────────────────────────── */

ZTEST(array, test_empty_array)
{
	void *pool = test_lit_pool_push();

	id arr = test_lit_array_empty();
	zassert_not_null(arr, "Empty array should not be nil");
	zassert_equal(test_lit_array_count(arr), 0, "Empty array count should be 0");

	test_lit_pool_pop(pool);
}

ZTEST(array, test_array_count)
{
	void *pool = test_lit_pool_push();

	id arr = test_lit_array_two();
	zassert_equal(test_lit_array_count(arr), 2, "Array should have 2 elements");

	test_lit_pool_pop(pool);
}

ZTEST(array, test_array_object_at_index)
{
	void *pool = test_lit_pool_push();

	id arr = test_lit_array_two();
	id first = test_lit_array_object_at(arr, 0);
	id second = test_lit_array_object_at(arr, 1);

	zassert_equal(test_lit_number_int_value(first), 1, "arr[0] should be 1");
	zassert_equal(test_lit_number_int_value(second), 2, "arr[1] should be 2");

	test_lit_pool_pop(pool);
}

ZTEST(array, test_array_out_of_bounds)
{
	void *pool = test_lit_pool_push();

	id arr = test_lit_array_two();
	id out = test_lit_array_object_at(arr, 99);
	zassert_is_null(out, "Out-of-bounds access should return nil");

	test_lit_pool_pop(pool);
}

ZTEST(array, test_array_subscript)
{
	void *pool = test_lit_pool_push();

	id arr = test_lit_array_two();
	id first = test_lit_array_subscript(arr, 0);
	zassert_equal(test_lit_number_int_value(first), 1, "arr[0] via subscript should be 1");

	test_lit_pool_pop(pool);
}

/* ── OZDictionary tests ───────────────────────────────────────── */

ZTEST(dictionary, test_empty_dict)
{
	void *pool = test_lit_pool_push();

	id d = test_lit_dict_empty();
	zassert_not_null(d, "Empty dict should not be nil");
	zassert_equal(test_lit_dict_count(d), 0, "Empty dict count should be 0");

	test_lit_pool_pop(pool);
}

ZTEST(dictionary, test_dict_count)
{
	void *pool = test_lit_pool_push();

	id d = test_lit_dict_one();
	zassert_equal(test_lit_dict_count(d), 1, "Dict should have 1 pair");

	test_lit_pool_pop(pool);
}

ZTEST(dictionary, test_dict_lookup_hit)
{
	void *pool = test_lit_pool_push();

	id d = test_lit_dict_one();
	id val = test_lit_dict_lookup_key(d);
	zassert_not_null(val, "objectForKey:@\"key\" should not be nil");
	zassert_equal(test_lit_number_int_value(val), 42, "dict[@\"key\"] should be 42");

	test_lit_pool_pop(pool);
}

ZTEST(dictionary, test_dict_lookup_miss)
{
	void *pool = test_lit_pool_push();

	id d = test_lit_dict_one();
	id val = test_lit_dict_lookup_missing(d);
	zassert_is_null(val, "objectForKey:@\"missing\" should be nil");

	test_lit_pool_pop(pool);
}

ZTEST(dictionary, test_dict_multi)
{
	void *pool = test_lit_pool_push();

	id d = test_lit_dict_multi();
	zassert_equal(test_lit_dict_count(d), 2, "Multi dict should have 2 pairs");

	id a = test_lit_dict_lookup_a(d);
	id b = test_lit_dict_lookup_b(d);
	zassert_equal(test_lit_number_int_value(a), 1, "dict[@\"a\"] should be 1");
	zassert_equal(test_lit_number_int_value(b), 2, "dict[@\"b\"] should be 2");

	test_lit_pool_pop(pool);
}

ZTEST(dictionary, test_dict_subscript)
{
	void *pool = test_lit_pool_push();

	id d = test_lit_dict_one();
	id val = test_lit_dict_subscript_key(d);
	zassert_not_null(val, "dict[@\"key\"] via subscript should not be nil");
	zassert_equal(test_lit_number_int_value(val), 42);

	test_lit_pool_pop(pool);
}
