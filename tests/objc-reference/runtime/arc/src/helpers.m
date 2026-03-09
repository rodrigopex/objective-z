/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief Helper classes for ARC tests.
 *
 * Compiled with ARC. Provides C-callable wrappers for the test harness.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <objc/arc.h>
#include <objc/runtime.h>

/* ── Dealloc tracking counter (read from C test code) ──────────── */

int g_arc_dealloc_count = 0;

/* ── ArcTestObj: subclass of Object ───────────────────────────── */

@interface ArcTestObj : Object
@end

@implementation ArcTestObj

- (void)dealloc
{
	g_arc_dealloc_count++;
}

@end

/* ── C-callable helpers ────────────────────────────────────────── */

__attribute__((ns_returns_retained))
id test_arc_create_obj(void)
{
	return [[ArcTestObj alloc] init];
}

unsigned int test_arc_get_rc(__unsafe_unretained id obj)
{
	return __objc_refcount_get(obj);
}

void test_arc_reset_count(void)
{
	g_arc_dealloc_count = 0;
}

void *test_arc_pool_push(void)
{
	return objc_autoreleasePoolPush();
}

void test_arc_pool_pop(void *p)
{
	objc_autoreleasePoolPop(p);
}

/* ── PropTestObj: for property accessor tests ──────────────────── */

@interface PropTestObj : Object {
@public
	__unsafe_unretained id _prop;
}
@end

@implementation PropTestObj
@end

__attribute__((ns_returns_retained))
id test_prop_create(void)
{
	return [[PropTestObj alloc] init];
}

ptrdiff_t test_prop_offset(void)
{
	PropTestObj *dummy = [[PropTestObj alloc] init];
	ptrdiff_t off = (char *)(void *)&dummy->_prop - (char *)(__bridge void *)dummy;
	return off;
}

void *test_prop_read_ivar(__unsafe_unretained id obj)
{
	return (__bridge void *)((PropTestObj *)obj)->_prop;
}

void test_prop_write_ivar(__unsafe_unretained id obj, __unsafe_unretained id val)
{
	((PropTestObj *)obj)->_prop = val;
}
