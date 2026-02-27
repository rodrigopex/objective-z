/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief MRR (non-ARC) helper classes for ARC tests.
 *
 * Compiled without -fobjc-arc so we can manually manage retain/release
 * and provide C-callable wrappers for the test harness.
 */
#import <objc/objc.h>
#import <objc/OZAutoreleasePool.h>

/* ── Dealloc tracking counter (read from C test code) ──────────── */

int g_arc_dealloc_count = 0;

/* ── ArcTestObj: subclass of Object ───────────────────────────── */

@interface ArcTestObj : Object
@end

@implementation ArcTestObj

- (void)dealloc
{
	g_arc_dealloc_count++;
	[super dealloc];
}

@end

/* ── C-callable helpers ────────────────────────────────────────── */

id test_arc_create_obj(void)
{
	return [[ArcTestObj alloc] init];
}

unsigned int test_arc_get_rc(id obj)
{
	return [obj retainCount];
}

void test_arc_reset_count(void)
{
	g_arc_dealloc_count = 0;
}

void *test_arc_pool_push(void)
{
	return [[OZAutoreleasePool alloc] init];
}

void test_arc_pool_pop(void *p)
{
	[(OZAutoreleasePool *)p drain];
}

/* ── PropTestObj: for property accessor tests ──────────────────── */

@interface PropTestObj : Object {
@public
	id _prop;
}
@end

@implementation PropTestObj

- (void)dealloc
{
	[_prop release];
	[super dealloc];
}

@end

id test_prop_create(void)
{
	return [[PropTestObj alloc] init];
}

ptrdiff_t test_prop_offset(void)
{
	PropTestObj *dummy = [[PropTestObj alloc] init];
	ptrdiff_t off = (char *)&dummy->_prop - (char *)dummy;
	[dummy release];
	return off;
}

id test_prop_read_ivar(id obj)
{
	return ((PropTestObj *)obj)->_prop;
}

void test_prop_write_ivar(id obj, id val)
{
	((PropTestObj *)obj)->_prop = val;
}
