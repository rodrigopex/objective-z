/* Fixed-point (Q31+shift) implementation for OZ transpiler. */

#import <Foundation/OZQ31.h>
#import <Foundation/OZLog.h>

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
/*
 * Integer-only Q31-to-string with configurable decimal precision.
 * No stdio, no float — pure integer math. Trailing zero removal.
 * precision: number of fractional digits (clamped to 0..14).
 */
static inline int _oz_q31_to_str(int32_t raw, uint8_t shift, char *buf, int maxLen,
				 int precision)
{
	if (maxLen <= 0) {
		return 0;
	}

	/* Clamp precision to valid range */
	if (precision < 0) {
		precision = 0;
	} else if (precision > 14) {
		precision = 14;
	}

	int pos = 0;

	if (raw == 0) {
		if (pos < maxLen) {
			buf[pos++] = '0';
		}
		return pos;
	}

	int neg = (raw < 0);
	uint32_t abs_raw = neg ? (uint32_t)(-(int64_t)raw) : (uint32_t)raw;

	uint8_t frac_bits = (shift >= 31) ? 0 : (31 - shift);
	uint32_t int_part = abs_raw >> frac_bits;
	uint32_t frac_mask = frac_bits ? (((uint32_t)1 << frac_bits) - 1) : 0;
	uint32_t frac_part = abs_raw & frac_mask;

	/* Generate precision + 1 fractional digits (extra one for rounding) */
	char frac_digits[15] = {0};
	int n_digits = precision + 1;
	if (n_digits > 15) {
		n_digits = 15;
	}
	uint64_t frac = (uint64_t)frac_part;
	if (frac_bits > 0) {
		for (int i = 0; i < n_digits; i++) {
			frac *= 10;
			frac_digits[i] = (char)(frac >> frac_bits);
			frac &= ((uint64_t)1 << frac_bits) - 1;
		}
	}

	/* Round at precision-th digit using the extra digit */
	if (precision > 0 && frac_digits[precision] >= 5) {
		int carry = 1;
		for (int i = precision - 1; i >= 0 && carry; i--) {
			int d = frac_digits[i] + carry;
			if (d >= 10) {
				frac_digits[i] = 0;
			} else {
				frac_digits[i] = (char)d;
				carry = 0;
			}
		}
		if (carry) {
			int_part++;
		}
	}

	/* Find last non-zero fractional digit */
	int last_frac = -1;
	for (int i = precision - 1; i >= 0; i--) {
		if (frac_digits[i] != 0) {
			last_frac = i;
			break;
		}
	}

	/* Write sign */
	if (neg && pos < maxLen) {
		buf[pos++] = '-';
	}

	/* Write integer part (reverse digit extraction) */
	char int_buf[12];
	int int_len = 0;
	if (int_part == 0) {
		int_buf[int_len++] = '0';
	} else {
		uint32_t tmp = int_part;
		while (tmp > 0) {
			int_buf[int_len++] = '0' + (char)(tmp % 10);
			tmp /= 10;
		}
	}
	for (int i = int_len - 1; i >= 0 && pos < maxLen; i--) {
		buf[pos++] = int_buf[i];
	}

	/* Write fractional part if any non-zero digits remain */
	if (last_frac >= 0) {
		if (pos < maxLen) {
			buf[pos++] = '.';
		}
		for (int i = 0; i <= last_frac && pos < maxLen; i++) {
			buf[pos++] = '0' + frac_digits[i];
		}
	}

	return pos;
}

/*
 * Integer-only Q31 division using 64-bit long division.
 * No float decode/encode — works entirely in Q31 domain.
 */
static inline void _oz_q31_div(int32_t a_raw, uint8_t a_shift,
				int32_t b_raw, uint8_t b_shift,
				int32_t *out_raw, uint8_t *out_shift)
{
	if (b_raw == 0) {
		*out_raw = 0;
		*out_shift = 0;
		return;
	}

	int neg = ((a_raw ^ b_raw) < 0) ? 1 : 0;
	uint64_t a = (a_raw < 0) ? (uint64_t)(-(int64_t)a_raw) : (uint64_t)a_raw;
	uint64_t b = (b_raw < 0) ? (uint64_t)(-(int64_t)b_raw) : (uint64_t)b_raw;

	/*
	 * Normalize both to Q31 (31 fractional bits):
	 * a_norm = a << a_shift, b_norm = b << b_shift
	 * Now value = x_norm / 2^31.
	 */
	uint64_t a_norm = a << a_shift;
	uint64_t b_norm = b << b_shift;

	/* Integer part of quotient */
	uint64_t q_int = a_norm / b_norm;
	uint64_t q_rem = a_norm % b_norm;

	/* Saturate if result too large for Q31 */
	if (q_int > (uint64_t)INT32_MAX) {
		*out_raw = neg ? INT32_MIN : INT32_MAX;
		*out_shift = 31;
		return;
	}

	uint8_t result_shift = _oz_bits_for_mag((uint32_t)q_int);
	uint8_t frac_bits = 31 - result_shift;

	/* Build result: integer part in upper bits */
	uint64_t result = q_int << frac_bits;

	/* Extract fractional bits via long division */
	for (int i = (int)frac_bits - 1; i >= 0; i--) {
		q_rem <<= 1;
		if (q_rem >= b_norm) {
			result |= ((uint64_t)1 << i);
			q_rem -= b_norm;
		}
	}

	/* Rounding: check next bit */
	q_rem <<= 1;
	if (q_rem >= b_norm) {
		result++;
	}

	if (result > (uint64_t)INT32_MAX) {
		result = INT32_MAX;
	}

	*out_raw = neg ? -(int32_t)result : (int32_t)result;
	*out_shift = result_shift;
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

	/*
	 * Re-normalizing add: if the sum overflows int32, shift right
	 * and increase the shift to preserve magnitude over precision.
	 */
	int64_t sum = (int64_t)a + (int64_t)b;
	while ((sum > INT32_MAX || sum < INT32_MIN) && s < 31) {
		sum >>= 1;
		s++;
	}
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

	/*
	 * Re-normalizing sub: same overflow handling as add.
	 */
	int64_t diff = (int64_t)a - (int64_t)b;
	while ((diff > INT32_MAX || diff < INT32_MIN) && s < 31) {
		diff >>= 1;
		s++;
	}
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
	 * Q31 integer-only division using 64-bit long division.
	 * No float intermediate — preserves full Q31 precision.
	 */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(_raw, _shift, other->_raw, other->_shift,
		    &r_raw, &r_shift);
	return [OZQ31 fixedWithRaw:r_raw shift:r_shift];
}

/* ── OZObject overrides ────────────────────────────────────────── */

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	int prec = _oz_get_log_precision();
	if (prec < 0) {
		prec = 14;
	}
	return _oz_q31_to_str(_raw, _shift, buf, maxLen, prec);
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
