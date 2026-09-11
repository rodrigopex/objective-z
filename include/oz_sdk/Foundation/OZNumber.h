/**
 * @file OZNumber.h
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

@interface OZNumber : OZObject {
	int32_t _raw;    /* Q31 mantissa, normalised to [-1.0, 1.0) */
	uint8_t _shift;  /* exponent: real_value = (raw / 2^31) * 2^shift */
}
/*
 * Factories. One family, not two: `fixedWith…` used to sit alongside
 * these with five members differing from their `numberWith…` twin in
 * nothing but the name, and `@42` desugared to the `fixedWith…` half
 * while user code was pointed at the other (#413).
 *
 * Widths are spelled out and *kept*: `UnsignedInt8` rather than Cocoa's
 * `UnsignedChar`. This is an embedded fixed-point type, not an NSNumber
 * clone -- what a caller needs to know is the width, and
 * `@compatibility_alias NSNumber` below is a convenience rather than a
 * contract.
 */
+ (instancetype)numberWithFloat:(float)value;
+ (instancetype)numberWithInt32:(int32_t)value;
/** @brief From an already-encoded Q-format value, as `sensor_decode` yields. */
+ (instancetype)numberWithRaw:(int32_t)raw shift:(uint8_t)shift;
+ (instancetype)numberWithInt8:(int8_t)value;
+ (instancetype)numberWithUnsignedInt8:(uint8_t)value;
+ (instancetype)numberWithInt16:(int16_t)value;
+ (instancetype)numberWithUnsignedInt16:(uint16_t)value;
+ (instancetype)numberWithUnsignedInt32:(uint32_t)value;
+ (instancetype)numberWithBool:(BOOL)value;
/*
 * The two platform-width forms, kept for NSNumber shape. They are the odd
 * ones out under the width rule above and are deliberately left alone:
 * nothing in the transpiler reaches for them (`@42` goes through
 * `numberWithInt32:`), so they cost only their own declaration.
 */
+ (instancetype)numberWithInt:(int)value;
+ (instancetype)numberWithUnsignedInt:(unsigned int)value;

/* Value extraction (Q31+shift -> target type) */
- (int8_t)int8Value;
- (uint8_t)unsignedInt8Value;
- (int16_t)int16Value;
- (uint16_t)unsignedInt16Value;
- (int32_t)int32Value;
- (uint32_t)unsignedInt32Value;
- (float)floatValue;
- (BOOL)boolValue;
- (int)intValue;
- (unsigned int)unsignedIntValue;

/* Q31 introspection (Zephyr sensor_decode interop) */
- (int32_t)rawValue;
- (uint8_t)shift;

/* Arithmetic (Q31 native) */
- (instancetype)adding:(OZNumber *)other;
- (instancetype)subtracting:(OZNumber *)other;
- (instancetype)multiplyingBy:(OZNumber *)other;
- (instancetype)dividingBy:(OZNumber *)other;

/* OZObject overrides */
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen;
- (BOOL)isEqual:(id)anObject;
@end

#ifdef __clang__
@compatibility_alias NSNumber OZNumber;
#endif
