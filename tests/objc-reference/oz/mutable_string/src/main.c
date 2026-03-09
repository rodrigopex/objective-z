/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for OZMutableString heap-backed dynamic buffer.
 */
#include <zephyr/ztest.h>
#include <string.h>

#include <objc/runtime.h>

/* ── Extern helpers from helpers.m ────────────────────────────────── */

extern void *test_mstr_pool_push(void);
extern void test_mstr_pool_pop(void *p);
extern id test_mstr_create(const char *str);
extern const char *test_mstr_cstr(id s);
extern unsigned int test_mstr_length(id s);
extern void test_mstr_append_cstr(id s, const char *str);
extern void test_mstr_append_string(id s, id other);
extern id test_mstr_oz_string_literal(void);

/* ── Test suites ──────────────────────────────────────────────────── */

ZTEST_SUITE(mstr_create, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(mstr_append, NULL, NULL, NULL, NULL, NULL);
ZTEST_SUITE(mstr_access, NULL, NULL, NULL, NULL, NULL);

/* ── stringWithCString tests ──────────────────────────────────────── */

ZTEST(mstr_create, test_create_normal)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("hello");
	zassert_not_null(s, "stringWithCString should return non-nil");
	zassert_equal(test_mstr_length(s), 5, "length should be 5");
	zassert_equal(strcmp(test_mstr_cstr(s), "hello"), 0, "cStr should match");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_create, test_create_null)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create(NULL);
	zassert_not_null(s, "stringWithCString:NULL should return non-nil");
	zassert_equal(test_mstr_length(s), 0, "length should be 0");
	zassert_equal(strcmp(test_mstr_cstr(s), ""), 0, "cStr should be empty");

	/* dealloc with _buf == NULL is exercised when pool drains */
	test_mstr_pool_pop(pool);
}

ZTEST(mstr_create, test_create_empty)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("");
	zassert_not_null(s, "stringWithCString:\"\" should return non-nil");
	zassert_equal(test_mstr_length(s), 0, "length should be 0");
	zassert_equal(strcmp(test_mstr_cstr(s), ""), 0, "cStr should be empty");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_create, test_create_long_string)
{
	void *pool = test_mstr_pool_push();

	/* 80 chars exceeds initial cap of 64, triggers doubling to 128 */
	char long_str[81];

	memset(long_str, 'A', 80);
	long_str[80] = '\0';

	id s = test_mstr_create(long_str);
	zassert_not_null(s, "long string should succeed");
	zassert_equal(test_mstr_length(s), 80, "length should be 80");
	zassert_equal(strcmp(test_mstr_cstr(s), long_str), 0, "content should match");

	test_mstr_pool_pop(pool);
}

/* ── appendCString tests ──────────────────────────────────────────── */

ZTEST(mstr_append, test_append_cstr_normal)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("foo");
	test_mstr_append_cstr(s, "bar");
	zassert_equal(test_mstr_length(s), 6, "length should be 6");
	zassert_equal(strcmp(test_mstr_cstr(s), "foobar"), 0, "content should be foobar");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_append, test_append_cstr_null)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("hello");
	test_mstr_append_cstr(s, NULL);
	zassert_equal(test_mstr_length(s), 5, "length should stay 5");
	zassert_equal(strcmp(test_mstr_cstr(s), "hello"), 0, "content unchanged");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_append, test_append_cstr_empty)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("hello");
	test_mstr_append_cstr(s, "");
	zassert_equal(test_mstr_length(s), 5, "length should stay 5");
	zassert_equal(strcmp(test_mstr_cstr(s), "hello"), 0, "content unchanged");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_append, test_append_cstr_triggers_realloc)
{
	void *pool = test_mstr_pool_push();

	/* Start with short string (cap=64), then append to exceed it */
	id s = test_mstr_create("start");

	char big[60];

	memset(big, 'X', 59);
	big[59] = '\0';

	test_mstr_append_cstr(s, big);
	zassert_equal(test_mstr_length(s), 5 + 59, "length should be 64");

	/* Verify content integrity after realloc */
	const char *result = test_mstr_cstr(s);

	zassert_equal(strncmp(result, "start", 5), 0, "prefix should be start");
	zassert_equal(result[5], 'X', "appended content should follow");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_append, test_append_cstr_to_null_buf)
{
	void *pool = test_mstr_pool_push();

	/* Create with NULL → _buf is NULL, _capacity is 0 */
	id s = test_mstr_create(NULL);

	zassert_equal(strcmp(test_mstr_cstr(s), ""), 0, "initially empty");

	/* Append triggers realloc(NULL, 64) → equivalent to malloc */
	test_mstr_append_cstr(s, "allocated");
	zassert_equal(test_mstr_length(s), 9, "length should be 9");
	zassert_equal(strcmp(test_mstr_cstr(s), "allocated"), 0, "content should match");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_append, test_append_cstr_multiple)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("(");

	test_mstr_append_cstr(s, "a");
	test_mstr_append_cstr(s, ", ");
	test_mstr_append_cstr(s, "b");
	test_mstr_append_cstr(s, ")");

	zassert_equal(test_mstr_length(s), 6, "length should be 6");
	zassert_equal(strcmp(test_mstr_cstr(s), "(a, b)"), 0, "content should match");

	test_mstr_pool_pop(pool);
}

/* ── appendString tests ───────────────────────────────────────────── */

ZTEST(mstr_append, test_append_string_normal)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("prefix_");
	id lit = test_mstr_oz_string_literal(); /* @"hello" */

	test_mstr_append_string(s, lit);
	zassert_equal(test_mstr_length(s), 12, "length should be 12");
	zassert_equal(strcmp(test_mstr_cstr(s), "prefix_hello"), 0, "content should match");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_append, test_append_string_nil)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("keep");
	test_mstr_append_string(s, nil);
	zassert_equal(test_mstr_length(s), 4, "length should stay 4");
	zassert_equal(strcmp(test_mstr_cstr(s), "keep"), 0, "content unchanged");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_append, test_append_string_mutable)
{
	void *pool = test_mstr_pool_push();

	id s1 = test_mstr_create("one");
	id s2 = test_mstr_create("two");

	test_mstr_append_string(s1, s2);
	zassert_equal(test_mstr_length(s1), 6, "length should be 6");
	zassert_equal(strcmp(test_mstr_cstr(s1), "onetwo"), 0, "content should match");

	test_mstr_pool_pop(pool);
}

/* ── cStr / length accessor tests ─────────────────────────────────── */

ZTEST(mstr_access, test_cstr_with_buf)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("test");
	const char *c = test_mstr_cstr(s);

	zassert_not_null(c, "cStr should not be NULL");
	zassert_equal(strcmp(c, "test"), 0, "cStr should return buffer content");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_access, test_cstr_without_buf)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create(NULL);
	const char *c = test_mstr_cstr(s);

	zassert_not_null(c, "cStr should not be NULL even with NULL buf");
	zassert_equal(strcmp(c, ""), 0, "cStr should return empty string");

	test_mstr_pool_pop(pool);
}

ZTEST(mstr_access, test_length_after_append)
{
	void *pool = test_mstr_pool_push();

	id s = test_mstr_create("ab");

	zassert_equal(test_mstr_length(s), 2, "initial length");
	test_mstr_append_cstr(s, "cd");
	zassert_equal(test_mstr_length(s), 4, "length after append");
	test_mstr_append_cstr(s, "efgh");
	zassert_equal(test_mstr_length(s), 8, "length after second append");

	test_mstr_pool_pop(pool);
}
