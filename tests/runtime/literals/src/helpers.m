/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper functions for the literals test suite.
 *
 * Compiled without -fobjc-arc via objz_target_sources().
 * Provides C-callable wrappers around ObjC literal syntax.
 */
#import <objc/objc.h>
#import <objc/OZAutoreleasePool.h>

/* ── Pool management ───────────────────────────────────────────── */

void *test_lit_pool_push(void)
{
	return [[OZAutoreleasePool alloc] init];
}

void test_lit_pool_pop(void *p)
{
	[(OZAutoreleasePool *)p drain];
}

/* ── OZNumber: creation ────────────────────────────────────────── */

id test_lit_bool_yes(void)
{
	return @YES;
}

id test_lit_bool_no(void)
{
	return @NO;
}

id test_lit_int(int v)
{
	return @(v);
}

id test_lit_double(double v)
{
	return @(v);
}

/* ── OZNumber: accessors ───────────────────────────────────────── */

BOOL test_lit_number_bool_value(id n)
{
	return [(OZNumber *)n boolValue];
}

int test_lit_number_int_value(id n)
{
	return [(OZNumber *)n intValue];
}

double test_lit_number_double_value(id n)
{
	return [(OZNumber *)n doubleValue];
}

unsigned int test_lit_number_hash(id n)
{
	return [(OZNumber *)n hash];
}

BOOL test_lit_number_is_equal(id a, id b)
{
	return [(OZNumber *)a isEqual:b];
}

unsigned int test_lit_number_retain_count(id n)
{
	return [(OZNumber *)n retainCount];
}

/* ── OZArray ───────────────────────────────────────────────────── */

id test_lit_array_empty(void)
{
	return @[];
}

id test_lit_array_two(void)
{
	return @[ @1, @2 ];
}

id test_lit_array_strings(void)
{
	return @[ @"alpha", @"beta", @"gamma" ];
}

unsigned int test_lit_array_count(id arr)
{
	return [(OZArray *)arr count];
}

id test_lit_array_object_at(id arr, unsigned int idx)
{
	return [(OZArray *)arr objectAtIndex:idx];
}

id test_lit_array_subscript(id arr, unsigned int idx)
{
	return ((OZArray *)arr)[idx];
}

/* ── OZDictionary ──────────────────────────────────────────────── */

id test_lit_dict_empty(void)
{
	return @{};
}

id test_lit_dict_one(void)
{
	return @{ @"key" : @42 };
}

id test_lit_dict_multi(void)
{
	return @{ @"a" : @1, @"b" : @2 };
}

unsigned int test_lit_dict_count(id d)
{
	return [(OZDictionary *)d count];
}

id test_lit_dict_object_for_key_str(id d, const char *key_cstr)
{
	/* Create an NXConstantString to compare; use @"" literal matching */
	/* We cannot construct arbitrary NXConstantString from C, so the
	 * caller must use one of the known keys tested below. */
	(void)key_cstr;
	return nil;
}

id test_lit_dict_lookup_key(id d)
{
	return [(OZDictionary *)d objectForKey:@"key"];
}

id test_lit_dict_lookup_a(id d)
{
	return [(OZDictionary *)d objectForKey:@"a"];
}

id test_lit_dict_lookup_b(id d)
{
	return [(OZDictionary *)d objectForKey:@"b"];
}

id test_lit_dict_lookup_missing(id d)
{
	return [(OZDictionary *)d objectForKey:@"missing"];
}

id test_lit_dict_subscript_key(id d)
{
	return ((OZDictionary *)d)[@"key"];
}
