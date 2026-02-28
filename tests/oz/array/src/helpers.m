/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper functions for the oz_array test suite.
 *
 * Compiled with ARC via objz_target_sources().
 * Provides C-callable wrappers around OZArray operations.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

#include <objc/arc.h>
#include <objc/runtime.h>
#include <string.h>

/* ── Pool management ──────────────────────────────────────────────── */

void *test_arr_pool_push(void)
{
	return objc_autoreleasePoolPush();
}

void test_arr_pool_pop(void *p)
{
	objc_autoreleasePoolPop(p);
}

/* ── Creation ─────────────────────────────────────────────────────── */

id test_arr_empty(void)
{
	return @[];
}

id test_arr_single(void)
{
	return @[ @42 ];
}

id test_arr_multi(void)
{
	return @[ @1, @2, @3 ];
}

id test_arr_strings(void)
{
	return @[ @"alpha", @"beta", @"gamma" ];
}

/* ── Accessors ────────────────────────────────────────────────────── */

unsigned int test_arr_count(id arr)
{
	return [(OZArray *)arr count];
}

id test_arr_object_at(id arr, unsigned int idx)
{
	return [(OZArray *)arr objectAtIndex:idx];
}

/* ── Description ──────────────────────────────────────────────────── */

const char *test_arr_description_cstr(id arr)
{
	id desc = [(OZArray *)arr description];
	return [(OZString *)desc cStr];
}

/* ── Retain count of element ──────────────────────────────────────── */

unsigned int test_arr_element_retain_count(id arr, unsigned int idx)
{
	__unsafe_unretained id obj = [(OZArray *)arr objectAtIndex:idx];
	return __objc_refcount_get(obj);
}

/* ── Number value helpers ─────────────────────────────────────────── */

int test_arr_int_value(id n)
{
	return [(OZNumber *)n intValue];
}

/* ── String value helpers ─────────────────────────────────────────── */

const char *test_arr_string_cstr(id s)
{
	return [(OZString *)s cStr];
}

/* ── Manual retain to test element lifecycle ───────────────────────── */

id test_arr_create_number(int v)
{
	return [OZNumber numberWithInt:v];
}

id test_arr_retain(__unsafe_unretained id obj)
{
	return objc_retain(obj);
}

unsigned int test_arr_retain_count(__unsafe_unretained id obj)
{
	return __objc_refcount_get(obj);
}

/* ── Fast enumeration ────────────────────────────────────────────── */

int test_arr_fast_enum_sum(id arr)
{
	int sum = 0;
	for (id obj in (OZArray *)arr) {
		sum += [(OZNumber *)obj intValue];
	}
	return sum;
}

unsigned int test_arr_fast_enum_count(id arr)
{
	unsigned int n = 0;
	for (id obj in (OZArray *)arr) {
		(void)obj;
		n++;
	}
	return n;
}
