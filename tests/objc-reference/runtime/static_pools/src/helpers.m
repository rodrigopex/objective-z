/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helpers for static pool tests.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <objc/arc.h>

/* ── TestPooled: class with a static memory pool ─────────────────── */

@interface TestPooled : Object {
	int _tag;
}
- (id)initWithTag:(int)tag;
- (int)tag;
@end

@implementation TestPooled

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

@end

/* ── TestUnpooled: class WITHOUT a static pool (heap fallback) ──── */

@interface TestUnpooled : Object {
	int _val;
}
- (id)initWithVal:(int)val;
- (int)val;
@end

@implementation TestUnpooled

- (id)initWithVal:(int)val
{
	self = [super init];
	if (self) {
		_val = val;
	}
	return self;
}

- (int)val
{
	return _val;
}

@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

__attribute__((ns_returns_retained)) id test_pool_create_pooled(int tag)
{
	return [[TestPooled alloc] initWithTag:tag];
}

int test_pool_get_tag(id obj)
{
	return [(TestPooled *)obj tag];
}

void test_pool_release_pooled(__unsafe_unretained id obj)
{
	objc_release(obj);
}

__attribute__((ns_returns_retained)) id test_pool_create_unpooled(int val)
{
	return [[TestUnpooled alloc] initWithVal:val];
}

int test_pool_get_val(id obj)
{
	return [(TestUnpooled *)obj val];
}

void test_pool_release_unpooled(__unsafe_unretained id obj)
{
	objc_release(obj);
}
