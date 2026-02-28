/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief MRR (non-ARC) helper classes for ARC intensive tests.
 *
 * Compiled without -fobjc-arc so we can manually manage retain/release
 * and provide C-callable wrappers for the test harness.
 */
#import <Foundation/Foundation.h>

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
	[super dealloc];
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
	[super dealloc];
}

@end

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

/* ── C-callable wrappers ─────────────────────────────────────────── */

id test_create_tracked(int tag)
{
	return [[TrackedObj alloc] initWithTag:tag];
}

unsigned int test_get_rc(id obj)
{
	return [obj retainCount];
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
	return [[OZAutoreleasePool alloc] init];
}

void test_pool_pop(void *p)
{
	[(OZAutoreleasePool *)p drain];
}

id test_create_pool_obj(int tag)
{
	return [[ArcPoolObj alloc] initWithTag:tag];
}

id test_get_immortal_string(void)
{
	return @"immortal test string";
}

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
