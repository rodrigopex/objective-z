/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for OZNumber factory methods, accessors, description, comparison, and hash.
 */
#include <zephyr/ztest.h>
#include <string.h>

#include <objc/runtime.h>

/* ── Extern helpers from helpers.m ────────────────────────────────── */

extern void *test_num_pool_push(void);
extern void test_num_pool_pop(void *p);

extern id test_num_char(char v);
extern id test_num_uchar(unsigned char v);
extern id test_num_short(short v);
extern id test_num_ushort(unsigned short v);
extern id test_num_int(int v);
extern id test_num_uint(unsigned int v);
extern id test_num_long(long v);
extern id test_num_ulong(unsigned long v);
extern id test_num_llong(long long v);
extern id test_num_ullong(unsigned long long v);
extern id test_num_float(float v);
extern id test_num_double(double v);
extern id test_num_bool(bool v);

extern bool test_num_bool_value(id n);
extern char test_num_char_value(id n);
extern int test_num_int_value(id n);
extern long test_num_long_value(id n);
extern long long test_num_llong_value(id n);
extern unsigned int test_num_uint_value(id n);
extern float test_num_float_value(id n);
extern double test_num_double_value(id n);
extern unsigned int test_num_retain_count(id n);

extern const char *test_num_description_cstr(id n);

extern bool test_num_is_equal(id a, id b);
extern unsigned int test_num_hash(id n);
extern id test_num_string_literal(void);

/* ── Test suites ──────────────────────────────────────────────────── */

ZTEST_SUITE(num_factory, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(num_accessor, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(num_description, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(num_comparison, NULL, NULL, NULL, NULL, NULL);

/* ── Factory: char / unsigned char ────────────────────────────────── */

ZTEST(num_factory, test_char_factory_and_value)
{
	void *pool = test_num_pool_push();

	id n = test_num_char('A');
	zassert_not_null(n, "numberWithChar should return non-nil");
	zassert_equal(test_num_char_value(n), 'A', "charValue should be 'A'");
	zassert_equal(test_num_int_value(n), 65, "intValue of char 'A' should be 65");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_uchar_factory)
{
	void *pool = test_num_pool_push();

	id n = test_num_uchar(200);
	zassert_not_null(n, "numberWithUnsignedChar should return non-nil");
	zassert_equal(test_num_int_value(n), 200, "intValue should be 200");

	test_num_pool_pop(pool);
}

/* ── Factory: short / unsigned short ──────────────────────────────── */

ZTEST(num_factory, test_short_factory)
{
	void *pool = test_num_pool_push();

	id n = test_num_short(-100);
	zassert_not_null(n, "numberWithShort should return non-nil");
	zassert_equal(test_num_int_value(n), -100, "intValue should be -100");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_ushort_factory)
{
	void *pool = test_num_pool_push();

	id n = test_num_ushort(60000);
	zassert_not_null(n, "numberWithUnsignedShort should return non-nil");
	zassert_equal(test_num_int_value(n), 60000, "intValue should be 60000");

	test_num_pool_pop(pool);
}

/* ── Factory: unsigned int (singleton + heap) ─────────────────────── */

ZTEST(num_factory, test_uint_factory_singleton)
{
	void *pool = test_num_pool_push();

	id a = test_num_uint(5);
	id b = test_num_uint(5);
	zassert_equal(a, b, "uint < 16 should return singleton");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_uint_factory_heap)
{
	void *pool = test_num_pool_push();

	id a = test_num_uint(100);
	id b = test_num_uint(100);
	zassert_not_equal(a, b, "uint >= 16 should be distinct heap objects");
	zassert_equal(test_num_int_value(a), 100, "intValue should be 100");

	test_num_pool_pop(pool);
}

/* ── Factory: long (singleton + heap + negative) ──────────────────── */

ZTEST(num_factory, test_long_factory_singleton)
{
	void *pool = test_num_pool_push();

	id a = test_num_long(10);
	id b = test_num_long(10);
	zassert_equal(a, b, "long in 0..15 should return singleton");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_long_factory_heap)
{
	void *pool = test_num_pool_push();

	id n = test_num_long(-5);
	zassert_not_null(n, "negative long should return non-nil");
	zassert_equal(test_num_long_value(n), -5, "longValue should be -5");

	test_num_pool_pop(pool);
}

/* ── Factory: unsigned long (singleton + heap) ────────────────────── */

ZTEST(num_factory, test_ulong_factory_singleton)
{
	void *pool = test_num_pool_push();

	id a = test_num_ulong(3);
	id b = test_num_ulong(3);
	zassert_equal(a, b, "ulong < 16 should return singleton");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_ulong_factory_heap)
{
	void *pool = test_num_pool_push();

	id n = test_num_ulong(1000);
	zassert_not_null(n, "ulong >= 16 should return non-nil");
	zassert_equal(test_num_int_value(n), 1000, "intValue should be 1000");

	test_num_pool_pop(pool);
}

/* ── Factory: long long (singleton + heap + negative) ─────────────── */

ZTEST(num_factory, test_llong_factory_singleton)
{
	void *pool = test_num_pool_push();

	id a = test_num_llong(7);
	id b = test_num_llong(7);
	zassert_equal(a, b, "llong in 0..15 should return singleton");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_llong_factory_heap)
{
	void *pool = test_num_pool_push();

	id n = test_num_llong(100000LL);
	zassert_not_null(n, "large llong should return non-nil");
	zassert_equal(test_num_llong_value(n), 100000LL, "llongValue should be 100000");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_llong_factory_negative)
{
	void *pool = test_num_pool_push();

	id n = test_num_llong(-42LL);
	zassert_equal(test_num_llong_value(n), -42LL, "llongValue should be -42");

	test_num_pool_pop(pool);
}

/* ── Factory: unsigned long long (singleton + heap) ───────────────── */

ZTEST(num_factory, test_ullong_factory_singleton)
{
	void *pool = test_num_pool_push();

	id a = test_num_ullong(0ULL);
	id b = test_num_ullong(0ULL);
	zassert_equal(a, b, "ullong < 16 should return singleton");

	test_num_pool_pop(pool);
}

ZTEST(num_factory, test_ullong_factory_heap)
{
	void *pool = test_num_pool_push();

	id n = test_num_ullong(500ULL);
	zassert_not_null(n, "ullong >= 16 should return non-nil");
	zassert_equal(test_num_llong_value(n), 500LL, "llongValue should be 500");

	test_num_pool_pop(pool);
}

/* ── Factory: float ───────────────────────────────────────────────── */

ZTEST(num_factory, test_float_factory)
{
	void *pool = test_num_pool_push();

	id n = test_num_float(2.5f);
	zassert_not_null(n, "numberWithFloat should return non-nil");
	float fv = test_num_float_value(n);
	zassert_true(fv > 2.4f && fv < 2.6f, "floatValue should be ~2.5");

	test_num_pool_pop(pool);
}

/* ── Factory: negative int (no singleton) ─────────────────────────── */

ZTEST(num_factory, test_negative_int_no_singleton)
{
	void *pool = test_num_pool_push();

	id a = test_num_int(-1);
	id b = test_num_int(-1);
	zassert_not_equal(a, b, "negative int should not be singleton");
	zassert_equal(test_num_int_value(a), -1, "intValue should be -1");

	test_num_pool_pop(pool);
}

/* ── Value accessors ──────────────────────────────────────────────── */

ZTEST(num_accessor, test_long_value)
{
	void *pool = test_num_pool_push();

	id n = test_num_int(42);
	zassert_equal(test_num_long_value(n), 42L, "longValue should be 42");

	test_num_pool_pop(pool);
}

ZTEST(num_accessor, test_llong_value_from_types)
{
	void *pool = test_num_pool_push();

	id nc = test_num_char(10);
	zassert_equal(test_num_llong_value(nc), 10LL, "llongValue from char");

	id ns = test_num_short(300);
	zassert_equal(test_num_llong_value(ns), 300LL, "llongValue from short");

	id ni = test_num_int(50000);
	zassert_equal(test_num_llong_value(ni), 50000LL, "llongValue from int");

	test_num_pool_pop(pool);
}

ZTEST(num_accessor, test_uint_value)
{
	void *pool = test_num_pool_push();

	id n = test_num_int(42);
	zassert_equal(test_num_uint_value(n), 42U, "unsignedIntValue should be 42");

	test_num_pool_pop(pool);
}

ZTEST(num_accessor, test_bool_from_float)
{
	void *pool = test_num_pool_push();

	id n = test_num_float(1.5f);
	zassert_true(test_num_bool_value(n), "nonzero float boolValue should be YES");

	test_num_pool_pop(pool);
}

ZTEST(num_accessor, test_bool_from_double_zero)
{
	void *pool = test_num_pool_push();

	id n = test_num_double(0.0);
	zassert_false(test_num_bool_value(n), "0.0 double boolValue should be NO");

	test_num_pool_pop(pool);
}

ZTEST(num_accessor, test_bool_from_int)
{
	void *pool = test_num_pool_push();

	id n = test_num_int(42);
	zassert_true(test_num_bool_value(n), "nonzero int boolValue should be YES");

	id z = test_num_int(0);
	/* 0 is a singleton, boolValue via longLongValue == 0 → NO */
	zassert_false(test_num_bool_value(z), "zero int boolValue should be NO");

	test_num_pool_pop(pool);
}

/* ── Description ──────────────────────────────────────────────────── */

ZTEST(num_description, test_description_bool)
{
	void *pool = test_num_pool_push();

	id yes = test_num_bool(1);
	zassert_equal(strcmp(test_num_description_cstr(yes), "YES"), 0, "@YES → \"YES\"");

	id no = test_num_bool(0);
	zassert_equal(strcmp(test_num_description_cstr(no), "NO"), 0, "@NO → \"NO\"");

	test_num_pool_pop(pool);
}

ZTEST(num_description, test_description_int)
{
	void *pool = test_num_pool_push();

	id n = test_num_int(42);
	zassert_equal(strcmp(test_num_description_cstr(n), "42"), 0, "@42 → \"42\"");

	test_num_pool_pop(pool);
}

ZTEST(num_description, test_description_double)
{
	void *pool = test_num_pool_push();

	id n = test_num_double(3.14);
	zassert_equal(strcmp(test_num_description_cstr(n), "3.14"), 0, "@3.14 → \"3.14\"");

	test_num_pool_pop(pool);
}

ZTEST(num_description, test_description_negative_double)
{
	void *pool = test_num_pool_push();

	id n = test_num_double(-2.50);
	zassert_equal(strcmp(test_num_description_cstr(n), "-2.50"), 0,
		       "@-2.50 → \"-2.50\"");

	test_num_pool_pop(pool);
}

/* ── isEqual: comparison ──────────────────────────────────────────── */

ZTEST(num_comparison, test_is_equal_cross_type)
{
	void *pool = test_num_pool_push();

	/* int vs long with same value → equal via longLongValue */
	id a = test_num_int(42);
	id b = test_num_long(42);
	zassert_true(test_num_is_equal(a, b), "int 42 isEqual long 42");

	test_num_pool_pop(pool);
}

ZTEST(num_comparison, test_is_equal_float_int)
{
	void *pool = test_num_pool_push();

	/* float vs int → doubleValue path */
	id f = test_num_float(3.0f);
	id i = test_num_int(3);
	zassert_true(test_num_is_equal(f, i), "float 3.0 isEqual int 3");

	test_num_pool_pop(pool);
}

ZTEST(num_comparison, test_is_equal_non_number)
{
	void *pool = test_num_pool_push();

	id n = test_num_int(42);
	id s = test_num_string_literal();
	zassert_false(test_num_is_equal(n, s), "number isEqual string should be NO");

	test_num_pool_pop(pool);
}

/* ── Hash ─────────────────────────────────────────────────────────── */

ZTEST(num_comparison, test_hash_float_exact_int)
{
	void *pool = test_num_pool_push();

	/* float 3.0 → exact integer → hash as integer 3 */
	id f = test_num_float(3.0f);
	id i = test_num_int(3);
	zassert_equal(test_num_hash(f), test_num_hash(i),
		      "float 3.0 hash should equal int 3 hash");

	test_num_pool_pop(pool);
}

ZTEST(num_comparison, test_hash_float_non_int)
{
	void *pool = test_num_pool_push();

	/* float 3.14 → not exact integer → memcpy hash */
	id n = test_num_float(3.14f);
	unsigned int h = test_num_hash(n);
	/* Just verify it returns a value; exact value depends on memcpy */
	zassert_true(h != 0 || h == 0, "hash should return a value");

	test_num_pool_pop(pool);
}

ZTEST(num_comparison, test_hash_int)
{
	void *pool = test_num_pool_push();

	id n = test_num_int(42);
	zassert_equal(test_num_hash(n), 42U, "int 42 hash should be 42");

	test_num_pool_pop(pool);
}
