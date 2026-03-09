/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief Helper classes for ARC intensive tests.
 *
 * Compiled with ARC. Provides C-callable wrappers for the test harness.
 */
#import <Foundation/Foundation.h>
#include <objc/arc.h>
#include <objc/runtime.h>

/* ── Dealloc tracking globals (read from C test code) ──────────── */

int g_dealloc_count = 0;
int g_dealloc_tags[64];
int g_dealloc_tag_idx = 0;

/* ── TrackedObj: Object subclass with dealloc tracking ─────────── */

@interface TrackedObj : Object {
	int _tag;
}
- (id)initWithTag:(int)tag;
- (int)tag;
@end

@implementation TrackedObj

- (id)initWithTag:(int)tag
{
	self = [super init];
	if (self) {
		_tag = tag;
	}
	return self;
}

- (int)tag
{
	return _tag;
}

- (void)dealloc
{
	g_dealloc_count++;
	if (g_dealloc_tag_idx < 64) {
		g_dealloc_tags[g_dealloc_tag_idx++] = _tag;
	}
}

@end

/* ── ArcPoolObj: static-pool-allocated class ───────────────────── */

@interface ArcPoolObj : Object {
	int _tag;
}
- (id)initWithTag:(int)tag;
- (int)tag;
@end

@implementation ArcPoolObj

- (id)initWithTag:(int)tag
{
	self = [super init];
	if (self) {
		_tag = tag;
	}
	return self;
}

- (int)tag
{
	return _tag;
}

- (void)dealloc
{
	g_dealloc_count++;
	if (g_dealloc_tag_idx < 64) {
		g_dealloc_tags[g_dealloc_tag_idx++] = _tag;
	}
}

@end

/* ── PropTestObj: for property accessor tests ──────────────────── */

@interface PropTestObj : Object {
@public
	__unsafe_unretained id _prop;
}
@end

@implementation PropTestObj
@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

__attribute__((ns_returns_retained))
id test_create_tracked(int tag)
{
	return [[TrackedObj alloc] initWithTag:tag];
}

unsigned int test_get_rc(__unsafe_unretained id obj)
{
	return __objc_refcount_get(obj);
}

void test_reset_tracking(void)
{
	g_dealloc_count = 0;
	g_dealloc_tag_idx = 0;
	for (int i = 0; i < 64; i++) {
		g_dealloc_tags[i] = 0;
	}
}

void *test_pool_push(void)
{
	return objc_autoreleasePoolPush();
}

void test_pool_pop(void *p)
{
	objc_autoreleasePoolPop(p);
}

__attribute__((ns_returns_retained))
id test_create_pool_obj(int tag)
{
	return [[ArcPoolObj alloc] initWithTag:tag];
}

id test_get_immortal_string(void)
{
	return @"immortal test string";
}

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
