/* Fixed-point (Q31+shift) implementation for OZ transpiler. */

#import <Foundation/OZQ31.h>
#include <stdio.h>

#ifndef _OZ_Q31_HELPERS
#define _OZ_Q31_HELPERS

static inline uint8_t _oz_bits_for_mag(uint32_t mag)
{
	if (mag == 0) {
		return 0;
	}
	int bits = 0;
	while (mag > 0) {
		mag >>= 1;
		bits++;
	}
	return (bits > 31) ? 31 : (uint8_t)bits;
}

static inline uint8_t _oz_shift_for_float(float value)
{
	if (value == 0.0f) {
		return 0;
	}
	float mag = (value < 0.0f) ? -value : value;
	return _oz_bits_for_mag((uint32_t)mag);
}

static inline uint8_t _oz_shift_for_int32(int32_t value)
{
	if (value == 0) {
		return 0;
	}
	uint32_t mag = (value < 0) ? (uint32_t)(-(int64_t)value) : (uint32_t)value;
	return _oz_bits_for_mag(mag);
}

static inline int32_t _oz_encode_float(float value, uint8_t shift)
{
	if (shift >= 31) {
		return (int32_t)(value * 0.5f);
	}
	return (int32_t)(value * (float)(1UL << (31 - shift)));
}

static inline int32_t _oz_encode_int32(int32_t value, uint8_t shift)
{
	return value << (31 - shift);
}

static inline float _oz_decode_float(int32_t raw, uint8_t shift)
{
	if (shift >= 31) {
		return (float)raw;
	}
	return (float)raw / (float)(1UL << (31 - shift));
}

static inline int32_t _oz_decode_int32(int32_t raw, uint8_t shift)
{
	if (shift >= 31) {
		return raw;
	}
	return raw >> (31 - shift);
}

static inline void _oz_align_shift(int32_t *raw_a, uint8_t shift_a,
				    int32_t *raw_b, uint8_t shift_b,
				    uint8_t *out_shift)
{
	if (shift_a == shift_b) {
		*out_shift = shift_a;
		return;
	}
	if (shift_a > shift_b) {
		*raw_b = *raw_b >> (shift_a - shift_b);
		*out_shift = shift_a;
	} else {
		*raw_a = *raw_a >> (shift_b - shift_a);
		*out_shift = shift_b;
	}
}
#endif /* _OZ_Q31_HELPERS */

@implementation OZQ31

+ (instancetype)fixedWithFloat:(float)value
{
	OZQ31 *fp = [[OZQ31 alloc] init];
	fp->_shift = _oz_shift_for_float(value);
	fp->_raw = _oz_encode_float(value, fp->_shift);
	return fp;
}

+ (instancetype)fixedWithInt32:(int32_t)value
{
	OZQ31 *fp = [[OZQ31 alloc] init];
	fp->_shift = _oz_shift_for_int32(value);
	fp->_raw = _oz_encode_int32(value, fp->_shift);
	return fp;
}

+ (instancetype)fixedWithRaw:(int32_t)raw shift:(uint8_t)shift
{
	OZQ31 *fp = [[OZQ31 alloc] init];
	fp->_raw = raw;
	fp->_shift = shift;
	return fp;
}

+ (instancetype)fixedWithBool:(BOOL)value
{
	return [OZQ31 fixedWithInt32:value ? 1 : 0];
}

+ (instancetype)fixedWithInt:(int)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

+ (instancetype)fixedWithUnsignedInt:(unsigned int)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

/* ── Clang literal compatibility (delegates to fixedWith*) ───── */

+ (instancetype)numberWithInt8:(int8_t)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

+ (instancetype)numberWithUint8:(uint8_t)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

+ (instancetype)numberWithInt16:(int16_t)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

+ (instancetype)numberWithUint16:(uint16_t)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

+ (instancetype)numberWithInt32:(int32_t)value
{
	return [OZQ31 fixedWithInt32:value];
}

+ (instancetype)numberWithUint32:(uint32_t)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

+ (instancetype)numberWithFloat:(float)value
{
	return [OZQ31 fixedWithFloat:value];
}

+ (instancetype)numberWithBool:(BOOL)value
{
	return [OZQ31 fixedWithBool:value];
}

+ (instancetype)numberWithInt:(int)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

+ (instancetype)numberWithUnsignedInt:(unsigned int)value
{
	return [OZQ31 fixedWithInt32:(int32_t)value];
}

/* ── Value extraction ──────────────────────────────────────────── */

- (int8_t)int8Value
{
	return (int8_t)_oz_decode_int32(_raw, _shift);
}

- (uint8_t)uint8Value
{
	return (uint8_t)_oz_decode_int32(_raw, _shift);
}

- (int16_t)int16Value
{
	return (int16_t)_oz_decode_int32(_raw, _shift);
}

- (uint16_t)uint16Value
{
	return (uint16_t)_oz_decode_int32(_raw, _shift);
}

- (int32_t)int32Value
{
	return _oz_decode_int32(_raw, _shift);
}

- (uint32_t)uint32Value
{
	return (uint32_t)_oz_decode_int32(_raw, _shift);
}

- (float)floatValue
{
	return _oz_decode_float(_raw, _shift);
}

- (BOOL)boolValue
{
	return _raw != 0;
}

- (int)intValue
{
	return (int)_oz_decode_int32(_raw, _shift);
}

- (unsigned int)unsignedIntValue
{
	return (unsigned int)_oz_decode_int32(_raw, _shift);
}

/* ── Q31 introspection ─────────────────────────────────────────── */

- (int32_t)rawValue
{
	return _raw;
}

- (uint8_t)shift
{
	return _shift;
}

/* ── Arithmetic ────────────────────────────────────────────────── */

- (instancetype)add:(OZQ31 *)other
{
	int32_t a = _raw;
	int32_t b = other->_raw;
	uint8_t s;
	_oz_align_shift(&a, _shift, &b, other->_shift, &s);

	/* Saturating add: clamp on overflow */
	int64_t sum = (int64_t)a + (int64_t)b;
	if (sum > INT32_MAX) {
		sum = INT32_MAX;
	}
	if (sum < INT32_MIN) {
		sum = INT32_MIN;
	}
	return [OZQ31 fixedWithRaw:(int32_t)sum shift:s];
}

- (instancetype)sub:(OZQ31 *)other
{
	int32_t a = _raw;
	int32_t b = other->_raw;
	uint8_t s;
	_oz_align_shift(&a, _shift, &b, other->_shift, &s);

	int64_t diff = (int64_t)a - (int64_t)b;
	if (diff > INT32_MAX) {
		diff = INT32_MAX;
	}
	if (diff < INT32_MIN) {
		diff = INT32_MIN;
	}
	return [OZQ31 fixedWithRaw:(int32_t)diff shift:s];
}

- (instancetype)mul:(OZQ31 *)other
{
	/*
	 * Q31 multiply:
	 * result_raw = (a_raw * b_raw) >> 31
	 * result_shift = a_shift + b_shift
	 *
	 * On Cortex-M4: maps to SMMUL instruction.
	 */
	int64_t product = (int64_t)_raw * (int64_t)other->_raw;
	int32_t result_raw = (int32_t)(product >> 31);
	uint8_t result_shift = _shift + other->_shift;
	if (result_shift > 31) {
		result_shift = 31;
	}
	return [OZQ31 fixedWithRaw:result_raw shift:result_shift];
}

- (instancetype)div:(OZQ31 *)other
{
	/*
	 * Q31 divide via float intermediate.
	 * Decode both to float, divide, re-encode.
	 * Guard against division by zero.
	 */
	if (other->_raw == 0) {
		return [OZQ31 fixedWithRaw:0 shift:0];
	}
	float a_val = _oz_decode_float(_raw, _shift);
	float b_val = _oz_decode_float(other->_raw, other->_shift);
	return [OZQ31 fixedWithFloat:a_val / b_val];
}

/* ── OZObject overrides ────────────────────────────────────────── */

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	float val = _oz_decode_float(_raw, _shift);
	return snprintf(buf, maxLen, "%g", (double)val);
}

- (BOOL)isEqual:(id)anObject
{
	if (self == anObject) {
		return YES;
	}
	OZQ31 *other = (OZQ31 *)anObject;
	return (_raw == other->_raw) && (_shift == other->_shift);
}

- (void)dealloc
{
	/* OZQ31 is a compile-time constant and must never be freed. */
}

@end
