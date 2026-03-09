/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper classes with categories for category tests.
 */
#import <Foundation/Object.h>
#include <objc/arc.h>

/* ── TestShape: base class ───────────────────────────────────────── */

@interface TestShape : Object {
	int _sides;
}
- (id)initWithSides:(int)sides;
- (int)sides;
- (int)baseValue;
+ (int)defaultSides;
@end

@implementation TestShape

- (id)initWithSides:(int)sides
{
	self = [super init];
	if (self) {
		_sides = sides;
	}
	return self;
}

- (int)sides
{
	return _sides;
}

- (int)baseValue
{
	return 100;
}

+ (int)defaultSides
{
	return 4;
}

@end

/* ── Category: TestShape (Geometry) — adds new methods ──────────── */

@interface TestShape (Geometry)
- (int)perimeter;
- (BOOL)isTriangle;
@end

@implementation TestShape (Geometry)

- (int)perimeter
{
	return _sides * 10;
}

- (BOOL)isTriangle
{
	return _sides == 3;
}

@end

/* ── Category: TestShape (Override) — overrides baseValue ────────── */

@interface TestShape (Override)
- (int)baseValue;
+ (int)defaultSides;
@end

@implementation TestShape (Override)

- (int)baseValue
{
	return 999;
}

+ (int)defaultSides
{
	return 6;
}

@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

__attribute__((ns_returns_retained)) id test_cat_create_shape(int sides)
{
	return [[TestShape alloc] initWithSides:sides];
}

int test_cat_sides(id obj)
{
	return [(TestShape *)obj sides];
}

int test_cat_base_value(id obj)
{
	return [(TestShape *)obj baseValue];
}

int test_cat_perimeter(id obj)
{
	return [(TestShape *)obj perimeter];
}

BOOL test_cat_is_triangle(id obj)
{
	return [(TestShape *)obj isTriangle];
}

int test_cat_default_sides(void)
{
	return [TestShape defaultSides];
}

void test_cat_dealloc(__unsafe_unretained id obj)
{
	objc_release(obj);
}

BOOL test_cat_responds_perimeter(void)
{
	return class_respondsToSelector([TestShape class], @selector(perimeter));
}

BOOL test_cat_responds_isTriangle(void)
{
	return class_respondsToSelector([TestShape class], @selector(isTriangle));
}
