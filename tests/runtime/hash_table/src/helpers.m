/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file helpers.m
 * @brief ObjC helper classes for hash table tests.
 *
 * Tests the method hash table indirectly by defining classes with
 * many methods and verifying dispatch and respondsToSelector results.
 */
#import <Foundation/Object.h>

/* ── TestCalc: class with many instance and class methods ────────── */

@interface TestCalc : Object {
	int _value;
}
- (id)initWithValue:(int)val;
- (int)value;
- (int)add:(int)n;
- (int)sub:(int)n;
- (int)mul:(int)n;
- (int)negate;
- (int)doubleValue;
- (int)tripleValue;
+ (int)classVersion;
+ (int)maxValue;
@end

@implementation TestCalc

- (id)initWithValue:(int)val
{
	self = [super init];
	if (self) {
		_value = val;
	}
	return self;
}

- (int)value
{
	return _value;
}

- (int)add:(int)n
{
	return _value + n;
}

- (int)sub:(int)n
{
	return _value - n;
}

- (int)mul:(int)n
{
	return _value * n;
}

- (int)negate
{
	return -_value;
}

- (int)doubleValue
{
	return _value * 2;
}

- (int)tripleValue
{
	return _value * 3;
}

+ (int)classVersion
{
	return 42;
}

+ (int)maxValue
{
	return 9999;
}

@end

/* ── TestCalcSub: subclass to test inherited method lookup ──────── */

@interface TestCalcSub : TestCalc
- (int)quadrupleValue;
@end

@implementation TestCalcSub

- (int)quadrupleValue
{
	return [self value] * 4;
}

@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

id test_hash_create_calc(int val)
{
	return [[TestCalc alloc] initWithValue:val];
}

id test_hash_create_calc_sub(int val)
{
	return [[TestCalcSub alloc] initWithValue:val];
}

int test_hash_calc_value(id obj)
{
	return [(TestCalc *)obj value];
}

int test_hash_calc_add(id obj, int n)
{
	return [(TestCalc *)obj add:n];
}

int test_hash_calc_sub(id obj, int n)
{
	return [(TestCalc *)obj sub:n];
}

int test_hash_calc_mul(id obj, int n)
{
	return [(TestCalc *)obj mul:n];
}

int test_hash_calc_negate(id obj)
{
	return [(TestCalc *)obj negate];
}

int test_hash_calc_double(id obj)
{
	return [(TestCalc *)obj doubleValue];
}

int test_hash_calc_triple(id obj)
{
	return [(TestCalc *)obj tripleValue];
}

int test_hash_calc_quadruple(id obj)
{
	return [(TestCalcSub *)obj quadrupleValue];
}

int test_hash_class_version(void)
{
	return [TestCalc classVersion];
}

int test_hash_class_max(void)
{
	return [TestCalc maxValue];
}

void test_hash_dealloc(id obj)
{
	[obj dealloc];
}

/* ── respondsToSelector helpers (need @selector in ObjC) ─────────── */

BOOL test_hash_instance_responds_value(void)
{
	return class_respondsToSelector([TestCalc class], @selector(value));
}

BOOL test_hash_instance_responds_classVersion(void)
{
	return class_respondsToSelector([TestCalc class], @selector(classVersion));
}

BOOL test_hash_metaclass_responds_classVersion(void)
{
	return class_metaclassRespondsToSelector([TestCalc class], @selector(classVersion));
}

BOOL test_hash_metaclass_responds_value(void)
{
	return class_metaclassRespondsToSelector([TestCalc class], @selector(value));
}

BOOL test_hash_responds_to_add(void)
{
	return class_respondsToSelector([TestCalc class], @selector(add:));
}

BOOL test_hash_responds_to_sub(void)
{
	return class_respondsToSelector([TestCalc class], @selector(sub:));
}

BOOL test_hash_responds_to_mul(void)
{
	return class_respondsToSelector([TestCalc class], @selector(mul:));
}

BOOL test_hash_responds_to_negate(void)
{
	return class_respondsToSelector([TestCalc class], @selector(negate));
}

BOOL test_hash_responds_to_doubleValue(void)
{
	return class_respondsToSelector([TestCalc class], @selector(doubleValue));
}

BOOL test_hash_responds_to_tripleValue(void)
{
	return class_respondsToSelector([TestCalc class], @selector(tripleValue));
}

BOOL test_hash_responds_to_nonexistent(void)
{
	return class_respondsToSelector([TestCalc class], @selector(nonExistentMethod));
}
