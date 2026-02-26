/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helpers for memory/Object/OZString tests.
 */
#import <objc/OZString.h>

/* ── TestItem: simple Object subclass with an ivar ───────────────── */

@interface TestItem : Object {
	int _data;
}
- (id)initWithData:(int)data;
- (int)data;
@end

@implementation TestItem

- (id)initWithData:(int)data
{
	self = [super init];
	if (self) {
		_data = data;
	}
	return self;
}

- (int)data
{
	return _data;
}

@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

id test_mem_create_item(int data)
{
	return [[TestItem alloc] initWithData:data];
}

int test_mem_item_data(id obj)
{
	return [(TestItem *)obj data];
}

void test_mem_dealloc(id obj)
{
	[obj dealloc];
}

id test_mem_create_object(void)
{
	return [[Object alloc] init];
}

BOOL test_mem_is_equal(id a, id b)
{
	return [a isEqual:b];
}

Class test_mem_get_class(id obj)
{
	return [obj class];
}

Class test_mem_get_superclass(id obj)
{
	return [obj superclass];
}

BOOL test_mem_responds_to_init(id obj)
{
	return [obj respondsToSelector:@selector(init)];
}

BOOL test_mem_responds_to_nonexistent(id obj)
{
	return [obj respondsToSelector:@selector(thisMethodDoesNotExist)];
}

/* ── OZString helpers ────────────────────────────────────── */

const char *test_mem_cstr_get(void)
{
	OZString *s = (OZString *)(id)@"hello";
	return [s cStr];
}

unsigned int test_mem_cstr_length(void)
{
	OZString *s = (OZString *)(id)@"hello";
	return [s length];
}

BOOL test_mem_cstr_equal_same(void)
{
	OZString *a = (OZString *)(id)@"hello";
	OZString *b = (OZString *)(id)@"hello";
	return [a isEqual:(id)b];
}

BOOL test_mem_cstr_equal_diff(void)
{
	OZString *a = (OZString *)(id)@"hello";
	OZString *b = (OZString *)(id)@"world";
	return [a isEqual:(id)b];
}

BOOL test_mem_cstr_identity(void)
{
	OZString *s = (OZString *)(id)@"hello";
	return [s isEqual:(id)s];
}
