/**
 * @file OZQ31.h
 * @brief Q31 fixed-point numeric class.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits pure-C fixed-point operations.
 * Internal storage: Q31 mantissa (always [-1.0, 1.0)) with a shift
 * exponent.  Real value = (raw / 2^31) * 2^shift.
 * Direct interop with Zephyr sensor_decode Q31+shift values.
 */
#pragma once
#import "OZObject.h"

@interface OZQ31 : OZObject {
	int32_t _raw;    /* Q31 mantissa, normalised to [-1.0, 1.0) */
	uint8_t _shift;  /* exponent: real_value = (raw / 2^31) * 2^shift */
}
/* Factory methods */
+ (instancetype)fixedWithFloat:(float)value;
+ (instancetype)fixedWithInt32:(int32_t)value;
+ (instancetype)fixedWithRaw:(int32_t)raw shift:(uint8_t)shift;
+ (instancetype)fixedWithBool:(BOOL)value;
/* Preferred factory for user code */
+ (instancetype)fixedWithInt:(int)value;
+ (instancetype)fixedWithUnsignedInt:(unsigned int)value;
/* Clang literal compatibility — Clang emits these for @42 / @42U / @3.14f */
+ (instancetype)numberWithInt8:(int8_t)value;
+ (instancetype)numberWithUint8:(uint8_t)value;
+ (instancetype)numberWithInt16:(int16_t)value;
+ (instancetype)numberWithUint16:(uint16_t)value;
+ (instancetype)numberWithInt32:(int32_t)value;
+ (instancetype)numberWithUint32:(uint32_t)value;
+ (instancetype)numberWithFloat:(float)value;
+ (instancetype)numberWithBool:(BOOL)value;
+ (instancetype)numberWithInt:(int)value;
+ (instancetype)numberWithUnsignedInt:(unsigned int)value;

/* Value extraction (Q31+shift -> target type) */
- (int8_t)int8Value;
- (uint8_t)uint8Value;
- (int16_t)int16Value;
- (uint16_t)uint16Value;
- (int32_t)int32Value;
- (uint32_t)uint32Value;
- (float)floatValue;
- (BOOL)boolValue;
- (int)intValue;
- (unsigned int)unsignedIntValue;

/* Q31 introspection (Zephyr sensor_decode interop) */
- (int32_t)rawValue;
- (uint8_t)shift;

/* Arithmetic (Q31 native) */
- (instancetype)add:(OZQ31 *)other;
- (instancetype)sub:(OZQ31 *)other;
- (instancetype)mul:(OZQ31 *)other;
- (instancetype)div:(OZQ31 *)other;

/* OZObject overrides */
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
- (BOOL)isEqual:(id)anObject;
@end

#ifdef __clang__
@compatibility_alias NSNumber OZQ31;
#endif
