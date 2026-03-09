/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper functions for the mutable_string test suite.
 *
 * Compiled with ARC via objz_target_sources().
 * Provides C-callable wrappers around OZMutableString operations.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

#include <objc/arc.h>
#include <string.h>

/* ── Pool management ──────────────────────────────────────────────── */

void *test_mstr_pool_push(void)
{
	return objc_autoreleasePoolPush();
}

void test_mstr_pool_pop(void *p)
{
	objc_autoreleasePoolPop(p);
}

/* ── Creation ─────────────────────────────────────────────────────── */

id test_mstr_create(const char *str)
{
	return [OZMutableString stringWithCString:str];
}

/* ── Accessors ────────────────────────────────────────────────────── */

const char *test_mstr_cstr(id s)
{
	return [(OZMutableString *)s cStr];
}

unsigned int test_mstr_length(id s)
{
	return [(OZMutableString *)s length];
}

/* ── Mutation ─────────────────────────────────────────────────────── */

void test_mstr_append_cstr(id s, const char *str)
{
	[(OZMutableString *)s appendCString:str];
}

void test_mstr_append_string(id s, id other)
{
	[(OZMutableString *)s appendString:other];
}

/* ── OZString literal helper ──────────────────────────────────────── */

id test_mstr_oz_string_literal(void)
{
	return @"hello";
}
