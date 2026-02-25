/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZNumber.m
 * @brief Boxed number class implementation.
 *
 * Singletons for @YES/@NO and small integers 0..15 are allocated in
 * +initialize with refcount set to INT32_MAX (immortal).
 */
#import <objc/OZNumber.h>
#import <objc/objc.h>
#include <stdint.h>
#include <string.h>
#include <zephyr/sys/printk.h>

#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

/* refcount.c */
extern void __objc_refcount_set(id obj, long value);

#define SMALL_INT_COUNT 16

static OZNumber *_boolNo;
static OZNumber *_boolYes;
static OZNumber *_smallInts[SMALL_INT_COUNT];

@implementation OZNumber

+ (void)initialize
{
	if (self != [OZNumber class]) {
		return;
	}

	/* Bool singletons */
	_boolNo = [[OZNumber alloc] init];
	_boolNo->_type = OZNumberTypeBool;
	_boolNo->_value.boolVal = NO;
	__objc_refcount_set((id)_boolNo, INT32_MAX);

	_boolYes = [[OZNumber alloc] init];
	_boolYes->_type = OZNumberTypeBool;
	_boolYes->_value.boolVal = YES;
	__objc_refcount_set((id)_boolYes, INT32_MAX);

	/* Small integer singletons 0..15 */
	for (int i = 0; i < SMALL_INT_COUNT; i++) {
		_smallInts[i] = [[OZNumber alloc] init];
		_smallInts[i]->_type = OZNumberTypeInt;
		_smallInts[i]->_value.intVal = i;
		__objc_refcount_set((id)_smallInts[i], INT32_MAX);
	}

	LOG_DBG("OZNumber singletons ready (2 bools + %d ints)", SMALL_INT_COUNT);
}

/* ── Factory methods ────────────────────────────────────────────── */

+ (id)numberWithBool:(BOOL)value
{
	return value ? (id)_boolYes : (id)_boolNo;
}

+ (id)numberWithChar:(char)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeChar;
	n->_value.charVal = value;
	return [n autorelease];
}

+ (id)numberWithUnsignedChar:(unsigned char)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeUnsignedChar;
	n->_value.ucharVal = value;
	return [n autorelease];
}

+ (id)numberWithShort:(short)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeShort;
	n->_value.shortVal = value;
	return [n autorelease];
}

+ (id)numberWithUnsignedShort:(unsigned short)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeUnsignedShort;
	n->_value.ushortVal = value;
	return [n autorelease];
}

+ (id)numberWithInt:(int)value
{
	if (value >= 0 && value < SMALL_INT_COUNT) {
		return (id)_smallInts[value];
	}
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeInt;
	n->_value.intVal = value;
	return [n autorelease];
}

+ (id)numberWithUnsignedInt:(unsigned int)value
{
	if (value < SMALL_INT_COUNT) {
		return (id)_smallInts[value];
	}
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeUnsignedInt;
	n->_value.uintVal = value;
	return [n autorelease];
}

+ (id)numberWithLong:(long)value
{
	if (value >= 0 && value < SMALL_INT_COUNT) {
		return (id)_smallInts[value];
	}
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeLong;
	n->_value.longVal = value;
	return [n autorelease];
}

+ (id)numberWithUnsignedLong:(unsigned long)value
{
	if (value < SMALL_INT_COUNT) {
		return (id)_smallInts[value];
	}
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeUnsignedLong;
	n->_value.ulongVal = value;
	return [n autorelease];
}

+ (id)numberWithLongLong:(long long)value
{
	if (value >= 0 && value < SMALL_INT_COUNT) {
		return (id)_smallInts[value];
	}
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeLongLong;
	n->_value.llongVal = value;
	return [n autorelease];
}

+ (id)numberWithUnsignedLongLong:(unsigned long long)value
{
	if (value < SMALL_INT_COUNT) {
		return (id)_smallInts[value];
	}
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeUnsignedLongLong;
	n->_value.ullongVal = value;
	return [n autorelease];
}

+ (id)numberWithFloat:(float)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeFloat;
	n->_value.floatVal = value;
	return [n autorelease];
}

+ (id)numberWithDouble:(double)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_type = OZNumberTypeDouble;
	n->_value.doubleVal = value;
	return [n autorelease];
}

/* ── Value accessors ────────────────────────────────────────────── */

- (BOOL)boolValue
{
	switch (_type) {
	case OZNumberTypeBool:
		return _value.boolVal;
	case OZNumberTypeFloat:
		return _value.floatVal != 0.0f;
	case OZNumberTypeDouble:
		return _value.doubleVal != 0.0;
	default:
		return [self longLongValue] != 0;
	}
}

- (char)charValue
{
	return (char)[self longLongValue];
}

- (int)intValue
{
	switch (_type) {
	case OZNumberTypeBool:
		return _value.boolVal ? 1 : 0;
	case OZNumberTypeChar:
		return (int)_value.charVal;
	case OZNumberTypeUnsignedChar:
		return (int)_value.ucharVal;
	case OZNumberTypeShort:
		return (int)_value.shortVal;
	case OZNumberTypeUnsignedShort:
		return (int)_value.ushortVal;
	case OZNumberTypeInt:
		return _value.intVal;
	case OZNumberTypeUnsignedInt:
		return (int)_value.uintVal;
	case OZNumberTypeLong:
		return (int)_value.longVal;
	case OZNumberTypeUnsignedLong:
		return (int)_value.ulongVal;
	case OZNumberTypeLongLong:
		return (int)_value.llongVal;
	case OZNumberTypeUnsignedLongLong:
		return (int)_value.ullongVal;
	case OZNumberTypeFloat:
		return (int)_value.floatVal;
	case OZNumberTypeDouble:
		return (int)_value.doubleVal;
	}
	return 0;
}

- (long)longValue
{
	return (long)[self longLongValue];
}

- (long long)longLongValue
{
	switch (_type) {
	case OZNumberTypeBool:
		return _value.boolVal ? 1LL : 0LL;
	case OZNumberTypeChar:
		return (long long)_value.charVal;
	case OZNumberTypeUnsignedChar:
		return (long long)_value.ucharVal;
	case OZNumberTypeShort:
		return (long long)_value.shortVal;
	case OZNumberTypeUnsignedShort:
		return (long long)_value.ushortVal;
	case OZNumberTypeInt:
		return (long long)_value.intVal;
	case OZNumberTypeUnsignedInt:
		return (long long)_value.uintVal;
	case OZNumberTypeLong:
		return (long long)_value.longVal;
	case OZNumberTypeUnsignedLong:
		return (long long)_value.ulongVal;
	case OZNumberTypeLongLong:
		return _value.llongVal;
	case OZNumberTypeUnsignedLongLong:
		return (long long)_value.ullongVal;
	case OZNumberTypeFloat:
		return (long long)_value.floatVal;
	case OZNumberTypeDouble:
		return (long long)_value.doubleVal;
	}
	return 0;
}

- (unsigned int)unsignedIntValue
{
	return (unsigned int)[self longLongValue];
}

- (float)floatValue
{
	return (float)[self doubleValue];
}

- (double)doubleValue
{
	switch (_type) {
	case OZNumberTypeBool:
		return _value.boolVal ? 1.0 : 0.0;
	case OZNumberTypeChar:
		return (double)_value.charVal;
	case OZNumberTypeUnsignedChar:
		return (double)_value.ucharVal;
	case OZNumberTypeShort:
		return (double)_value.shortVal;
	case OZNumberTypeUnsignedShort:
		return (double)_value.ushortVal;
	case OZNumberTypeInt:
		return (double)_value.intVal;
	case OZNumberTypeUnsignedInt:
		return (double)_value.uintVal;
	case OZNumberTypeLong:
		return (double)_value.longVal;
	case OZNumberTypeUnsignedLong:
		return (double)_value.ulongVal;
	case OZNumberTypeLongLong:
		return (double)_value.llongVal;
	case OZNumberTypeUnsignedLongLong:
		return (double)_value.ullongVal;
	case OZNumberTypeFloat:
		return (double)_value.floatVal;
	case OZNumberTypeDouble:
		return _value.doubleVal;
	}
	return 0.0;
}

/* ── Comparison ─────────────────────────────────────────────────── */

- (BOOL)isEqual:(id)other
{
	if (self == other) {
		return YES;
	}
	if (![other isKindOfClass:[OZNumber class]]) {
		return NO;
	}
	OZNumber *num = (OZNumber *)other;

	/* Compare via doubleValue when either operand is floating point */
	if (_type == OZNumberTypeFloat || _type == OZNumberTypeDouble ||
	    num->_type == OZNumberTypeFloat || num->_type == OZNumberTypeDouble) {
		return [self doubleValue] == [num doubleValue];
	}
	/* Integer types: compare via longLongValue */
	return [self longLongValue] == [num longLongValue];
}

- (unsigned int)hash
{
	if (_type == OZNumberTypeFloat || _type == OZNumberTypeDouble) {
		double d = [self doubleValue];
		/* If the double is an exact integer, hash as integer for consistency */
		long long ll = (long long)d;
		if ((double)ll == d) {
			return (unsigned int)ll;
		}
		unsigned int h;
		memcpy(&h, &d, sizeof(h));
		return h;
	}
	return (unsigned int)[self longLongValue];
}

@end
