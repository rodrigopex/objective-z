/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for OZString compile-time constant string class.
 */
#include <zephyr/ztest.h>
#include <string.h>

#include <objc/runtime.h>

/* ── Extern helpers from helpers.m ────────────────────────────────── */

extern void *test_str_pool_push(void);
extern void test_str_pool_pop(void *p);
extern id test_str_alloc(void);
extern id test_str_literal_hello(void);
extern id test_str_literal_empty(void);
extern id test_str_literal_world(void);
extern id test_str_literal_hell(void);
extern const char *test_str_cstr(id s);
extern unsigned int test_str_length(id s);
extern id test_str_description(id s);
extern id test_str_retain(id s);
extern void test_str_release(id s);
extern bool test_str_is_equal(id a, id b);
extern bool test_str_is_equal_number(id s, id n);

/* ── Test suites ──────────────────────────────────────────────────── */

ZTEST_SUITE(oz_string, NULL, NULL, NULL, NULL, NULL);

/* ── Allocation ───────────────────────────────────────────────────── */

ZTEST(oz_string, test_alloc_returns_nil)
{
	id s = test_str_alloc();
	zassert_is_null(s, "+alloc should return nil for OZString");
}

/* ── cStr and length ──────────────────────────────────────────────── */

ZTEST(oz_string, test_cstr_and_length)
{
	id s = test_str_literal_hello();
	zassert_not_null(s, "@\"hello\" should not be nil");
	zassert_equal(test_str_length(s), 5, "length should be 5");
	zassert_equal(strcmp(test_str_cstr(s), "hello"), 0, "cStr should be \"hello\"");
}

ZTEST(oz_string, test_empty_string)
{
	id s = test_str_literal_empty();
	zassert_not_null(s, "@\"\" should not be nil");
	zassert_equal(test_str_length(s), 0, "length should be 0");
	zassert_equal(strcmp(test_str_cstr(s), ""), 0, "cStr should be empty");
}

/* ── description ──────────────────────────────────────────────────── */

ZTEST(oz_string, test_description_returns_self)
{
	id s = test_str_literal_hello();
	id desc = test_str_description(s);
	zassert_equal(s, desc, "-description should return self");
}

/* ── retain / release ─────────────────────────────────────────────── */

ZTEST(oz_string, test_retain_returns_self)
{
	id s = test_str_literal_hello();
	id r = test_str_retain(s);
	zassert_equal(s, r, "-retain should return self");
}

ZTEST(oz_string, test_release_noop)
{
	id s = test_str_literal_hello();
	/* Should not crash or change anything */
	test_str_release(s);
	zassert_equal(test_str_length(s), 5, "string should remain valid after release");
}

/* ── isEqual: ─────────────────────────────────────────────────────── */

ZTEST(oz_string, test_is_equal_identity)
{
	id s = test_str_literal_hello();
	zassert_true(test_str_is_equal(s, s), "self == self should be YES");
}

ZTEST(oz_string, test_is_equal_same_content)
{
	id a = test_str_literal_hello();
	id b = test_str_literal_hello();
	zassert_true(test_str_is_equal(a, b), "same literal should be equal");
}

ZTEST(oz_string, test_is_equal_different_length)
{
	id a = test_str_literal_hello();
	id b = test_str_literal_hell();
	zassert_false(test_str_is_equal(a, b), "different length should not be equal");
}

ZTEST(oz_string, test_is_equal_different_content)
{
	id a = test_str_literal_hello();
	id b = test_str_literal_world();
	zassert_false(test_str_is_equal(a, b), "different content should not be equal");
}

ZTEST(oz_string, test_is_equal_different_class)
{
	void *pool = test_str_pool_push();

	id s = test_str_literal_hello();
	/* Passing nil exercises the class comparison branch — nil has no class */
	zassert_false(test_str_is_equal(s, nil), "non-OZString should not be equal");

	test_str_pool_pop(pool);
}
