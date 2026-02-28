/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper functions for the oz_number test suite.
 *
 * Compiled without -fobjc-arc via objz_target_sources().
 * Provides C-callable wrappers around OZNumber operations.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

#include <string.h>

/* ── Pool management ──────────────────────────────────────────────── */

void *test_num_pool_push(void)
{
	return [[OZAutoreleasePool alloc] init];
}

void test_num_pool_pop(void *p)
{
	[(OZAutoreleasePool *)p drain];
}

/* ── Factory methods ──────────────────────────────────────────────── */

id test_num_char(char v)
{
	return [OZNumber numberWithChar:v];
}

id test_num_uchar(unsigned char v)
{
	return [OZNumber numberWithUnsignedChar:v];
}

id test_num_short(short v)
{
	return [OZNumber numberWithShort:v];
}

id test_num_ushort(unsigned short v)
{
	return [OZNumber numberWithUnsignedShort:v];
}

id test_num_int(int v)
{
	return [OZNumber numberWithInt:v];
}

id test_num_uint(unsigned int v)
{
	return [OZNumber numberWithUnsignedInt:v];
}

id test_num_long(long v)
{
	return [OZNumber numberWithLong:v];
}

id test_num_ulong(unsigned long v)
{
	return [OZNumber numberWithUnsignedLong:v];
}

id test_num_llong(long long v)
{
	return [OZNumber numberWithLongLong:v];
}

id test_num_ullong(unsigned long long v)
{
	return [OZNumber numberWithUnsignedLongLong:v];
}

id test_num_float(float v)
{
	return [OZNumber numberWithFloat:v];
}

id test_num_double(double v)
{
	return [OZNumber numberWithDouble:v];
}

id test_num_bool(BOOL v)
{
	return [OZNumber numberWithBool:v];
}

/* ── Value accessors ──────────────────────────────────────────────── */

BOOL test_num_bool_value(id n)
{
	return [(OZNumber *)n boolValue];
}

char test_num_char_value(id n)
{
	return [(OZNumber *)n charValue];
}

int test_num_int_value(id n)
{
	return [(OZNumber *)n intValue];
}

long test_num_long_value(id n)
{
	return [(OZNumber *)n longValue];
}

long long test_num_llong_value(id n)
{
	return [(OZNumber *)n longLongValue];
}

unsigned int test_num_uint_value(id n)
{
	return [(OZNumber *)n unsignedIntValue];
}

float test_num_float_value(id n)
{
	return [(OZNumber *)n floatValue];
}

double test_num_double_value(id n)
{
	return [(OZNumber *)n doubleValue];
}

unsigned int test_num_retain_count(id n)
{
	return [(OZNumber *)n retainCount];
}

/* ── Description ──────────────────────────────────────────────────── */

const char *test_num_description_cstr(id n)
{
	id desc = [(OZNumber *)n description];
	return [(OZString *)desc cStr];
}

/* ── Comparison ───────────────────────────────────────────────────── */

BOOL test_num_is_equal(id a, id b)
{
	return [(OZNumber *)a isEqual:b];
}

unsigned int test_num_hash(id n)
{
	return [(OZNumber *)n hash];
}

/* ── String literal for cross-class comparison ────────────────────── */

id test_num_string_literal(void)
{
	return @"hello";
}
