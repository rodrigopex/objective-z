/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper classes for dispatch cache tests.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

/* ── CacheBase: direct methods (depth=0) ─────────────────────────── */

@interface CacheBase : Object
- (int)value;
- (int)shared;
+ (int)classValue;
@end

@implementation CacheBase

- (int)value
{
	return 10;
}

- (int)shared
{
	return 100;
}

+ (int)classValue
{
	return 42;
}

@end

/* ── CacheChild: inherits from CacheBase (depth=1) ───────────────── */

@interface CacheChild : CacheBase
- (int)childOnly;
@end

@implementation CacheChild

- (int)childOnly
{
	return 20;
}

@end

/* ── CacheGrandChild: inherits from CacheChild (depth=2) ─────────── */

@interface CacheGrandChild : CacheChild
@end

@implementation CacheGrandChild
@end

/* ── CachePeer: separate class with same selector names ──────────── */

@interface CachePeer : Object
- (int)value;
- (int)shared;
@end

@implementation CachePeer

- (int)value
{
	return 77;
}

- (int)shared
{
	return 200;
}

@end

/* ── Category that overrides CacheBase -shared ────────────────────── */

@interface CacheBase (Override)
- (int)shared;
@end

@implementation CacheBase (Override)

- (int)shared
{
	return 999;
}

@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

id test_cache_create_base(void)
{
	return [[CacheBase alloc] init];
}

id test_cache_create_child(void)
{
	return [[CacheChild alloc] init];
}

id test_cache_create_grandchild(void)
{
	return [[CacheGrandChild alloc] init];
}

id test_cache_create_peer(void)
{
	return [[CachePeer alloc] init];
}

void test_cache_dealloc(id obj)
{
	[obj dealloc];
}

int test_cache_call_value(id obj)
{
	return [obj value];
}

int test_cache_call_shared(id obj)
{
	return [obj shared];
}

int test_cache_call_class_value(void)
{
	return [CacheBase classValue];
}

int test_cache_call_child_only(id obj)
{
	return [obj childOnly];
}
