/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper functions for the oz_dictionary test suite.
 *
 * Compiled with ARC via objz_target_sources().
 * Provides C-callable wrappers around OZDictionary operations.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

#include <objc/arc.h>
#include <string.h>

/* ── Pool management ──────────────────────────────────────────────── */

void *test_dict_pool_push(void)
{
	return objc_autoreleasePoolPush();
}

void test_dict_pool_pop(void *p)
{
	objc_autoreleasePoolPop(p);
}

/* ── Creation ─────────────────────────────────────────────────────── */

id test_dict_empty(void)
{
	return @{};
}

id test_dict_single(void)
{
	return @{@"name" : @"Alice"};
}

id test_dict_multi(void)
{
	return @{@"x" : @10, @"y" : @20, @"z" : @30};
}

id test_dict_number_keys(void)
{
	return @{@1 : @"one", @2 : @"two"};
}

/* ── Accessors ────────────────────────────────────────────────────── */

unsigned int test_dict_count(id d)
{
	return [(OZDictionary *)d count];
}

/* ── Lookup with string keys ──────────────────────────────────────── */

id test_dict_lookup_name(id d)
{
	return [(OZDictionary *)d objectForKey:@"name"];
}

id test_dict_lookup_x(id d)
{
	return [(OZDictionary *)d objectForKey:@"x"];
}

id test_dict_lookup_y(id d)
{
	return [(OZDictionary *)d objectForKey:@"y"];
}

id test_dict_lookup_z(id d)
{
	return [(OZDictionary *)d objectForKey:@"z"];
}

id test_dict_lookup_missing(id d)
{
	return [(OZDictionary *)d objectForKey:@"missing"];
}

/* ── Lookup with number keys ──────────────────────────────────────── */

id test_dict_lookup_num_key(id d, int key)
{
	id numKey = [OZNumber numberWithInt:key];
	return [(OZDictionary *)d objectForKey:numKey];
}

/* ── Description ──────────────────────────────────────────────────── */

const char *test_dict_description_cstr(id d)
{
	id desc = [(OZDictionary *)d description];
	return [(OZString *)desc cStr];
}

/* ── Value helpers ────────────────────────────────────────────────── */

int test_dict_int_value(id n)
{
	return [(OZNumber *)n intValue];
}

const char *test_dict_string_cstr(id s)
{
	return [(OZString *)s cStr];
}
