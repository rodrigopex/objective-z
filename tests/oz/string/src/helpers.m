/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper functions for the oz_string test suite.
 *
 * Compiled without -fobjc-arc via objz_target_sources().
 * Provides C-callable wrappers around OZString operations.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

#include <string.h>

/* ── Pool management ──────────────────────────────────────────────── */

void *test_str_pool_push(void)
{
	return [[OZAutoreleasePool alloc] init];
}

void test_str_pool_pop(void *p)
{
	[(OZAutoreleasePool *)p drain];
}

/* ── Creation ─────────────────────────────────────────────────────── */

id test_str_alloc(void)
{
	return [OZString alloc];
}

id test_str_literal_hello(void)
{
	return @"hello";
}

id test_str_literal_empty(void)
{
	return @"";
}

id test_str_literal_world(void)
{
	return @"world";
}

id test_str_literal_hell(void)
{
	return @"hell";
}

/* ── Accessors ────────────────────────────────────────────────────── */

const char *test_str_cstr(id s)
{
	return [(OZString *)s cStr];
}

unsigned int test_str_length(id s)
{
	return [(OZString *)s length];
}

id test_str_description(id s)
{
	return [(OZString *)s description];
}

id test_str_retain(id s)
{
	return [(OZString *)s retain];
}

void test_str_release(id s)
{
	[(OZString *)s release];
}

/* ── Comparison ───────────────────────────────────────────────────── */

BOOL test_str_is_equal(id a, id b)
{
	return [(OZString *)a isEqual:b];
}

BOOL test_str_is_equal_number(id s, id n)
{
	return [(OZString *)s isEqual:n];
}
