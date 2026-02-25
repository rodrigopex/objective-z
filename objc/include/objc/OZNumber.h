/**
 * @file OZNumber.h
 * @brief Boxed number class for ObjC literal support.
 *
 * Provides OZNumber for @42, @YES, @3.14 boxed literals.
 * Boolean and small integer (0..15) singletons are cached.
 * Aliased to NSNumber under Clang for compiler literal codegen.
 */
#pragma once
#import <objc/OZObject.h>

enum oz_number_type {
	OZNumberTypeBool,
	OZNumberTypeChar,
	OZNumberTypeUnsignedChar,
	OZNumberTypeShort,
	OZNumberTypeUnsignedShort,
	OZNumberTypeInt,
	OZNumberTypeUnsignedInt,
	OZNumberTypeLong,
	OZNumberTypeUnsignedLong,
	OZNumberTypeLongLong,
	OZNumberTypeUnsignedLongLong,
	OZNumberTypeFloat,
	OZNumberTypeDouble,
};

/**
 * @brief Boxed number with type-preserving storage.
 * @headerfile OZNumber.h objc/OZNumber.h
 * @ingroup objc
 *
 * Wraps C numeric types in an OZObject for use with ObjC boxed
 * literals (@42, @YES, @3.14). Boolean and small integer values
 * are returned as immortal singletons.
 */
@interface OZNumber : OZObject {
	enum oz_number_type _type;
	union {
		BOOL boolVal;
		char charVal;
		unsigned char ucharVal;
		short shortVal;
		unsigned short ushortVal;
		int intVal;
		unsigned int uintVal;
		long longVal;
		unsigned long ulongVal;
		long long llongVal;
		unsigned long long ullongVal;
		float floatVal;
		double doubleVal;
	} _value;
}

/** @name Factory methods (called by compiler for boxed literals) */

+ (id)numberWithBool:(BOOL)value;
+ (id)numberWithChar:(char)value;
+ (id)numberWithUnsignedChar:(unsigned char)value;
+ (id)numberWithShort:(short)value;
+ (id)numberWithUnsignedShort:(unsigned short)value;
+ (id)numberWithInt:(int)value;
+ (id)numberWithUnsignedInt:(unsigned int)value;
+ (id)numberWithLong:(long)value;
+ (id)numberWithUnsignedLong:(unsigned long)value;
+ (id)numberWithLongLong:(long long)value;
+ (id)numberWithUnsignedLongLong:(unsigned long long)value;
+ (id)numberWithFloat:(float)value;
+ (id)numberWithDouble:(double)value;

/** @name Value accessors */

- (BOOL)boolValue;
- (char)charValue;
- (int)intValue;
- (long)longValue;
- (long long)longLongValue;
- (unsigned int)unsignedIntValue;
- (float)floatValue;
- (double)doubleValue;

/** @name Comparison */

- (BOOL)isEqual:(id)other;
- (unsigned int)hash;

@end

#ifdef __clang__
@compatibility_alias NSNumber OZNumber;
#endif
