// SPDX-License-Identifier: Apache-2.0
//
// common/mod.rs - shared test helper: transpile, compile with the real
// PAL host headers, link, and run. Validates the static subset produces
// real, working C -- not just text that merely parses.
//
// Each test binary in tests/ compiles this module separately and uses
// only a subset of it, so per-binary "never used" warnings are expected.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// `tools/oz_static/../../include` -- the repo's real platform headers.
fn include_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include")
}

/// Transpile `source`, compile the primary output + companion file against
/// the real PAL (host backend), link, run, and return captured stdout.
/// Panics with full diagnostics/compiler output on any failure.
pub fn compile_and_run(source: &str, stem: &str) -> String {
    let out = oz_static::transpile(source).unwrap_or_else(|diags| {
        panic!(
            "transpile('{}') was expected to succeed but produced diagnostics:\n{}",
            stem,
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });

    let dir = std::env::temp_dir().join(format!("oz_static_test_{}", stem));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let main_c = dir.join(format!("{}.c", stem));
    fs::write(&main_c, &out.source_c).unwrap();
    fs::write(dir.join("oz_static_dispatch.h"), &out.companion_h).unwrap();
    let dispatch_c = dir.join("oz_static_dispatch.c");
    fs::write(&dispatch_c, &out.companion_c).unwrap();

    let main_o = dir.join("main.o");
    let dispatch_o = dir.join("dispatch.o");
    let bin = dir.join("bin");

    cc(&["-DOZ_PLATFORM_HOST", "-I", include_dir().to_str().unwrap(), "-I",
         dir.to_str().unwrap(), "-c", main_c.to_str().unwrap(), "-o", main_o.to_str().unwrap()]);
    cc(&["-DOZ_PLATFORM_HOST", "-I", include_dir().to_str().unwrap(), "-I",
         dir.to_str().unwrap(), "-c", dispatch_c.to_str().unwrap(), "-o", dispatch_o.to_str().unwrap()]);
    cc(&[main_o.to_str().unwrap(), dispatch_o.to_str().unwrap(), "-o", bin.to_str().unwrap()]);

    let run = Command::new(&bin).output().unwrap_or_else(|e| panic!("failed to run binary: {}", e));
    assert!(run.status.success(), "binary exited non-zero: {:?}\nstdout: {}\nstderr: {}",
            run.status, String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));

    String::from_utf8(run.stdout).unwrap()
}

fn cc(args: &[&str]) {
    let output = Command::new("cc").args(args).output().unwrap_or_else(|e| panic!("failed to run cc: {}", e));
    assert!(
        output.status.success(),
        "cc {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Transpile `source`, expecting it to be rejected. Returns the joined
/// diagnostic messages for substring assertions.
pub fn expect_reject(source: &str) -> String {
    match oz_static::transpile(source) {
        Ok(_) => panic!("expected transpile to be rejected by the static bar, but it succeeded"),
        Err(diags) => diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"),
    }
}

/// OZQ31, the fixed-point Foundation class, transplanted verbatim from the
/// real `src/OZQ31.m` / `include/oz_sdk/Foundation/OZQ31.h` (same helper
/// function bodies and same method bodies, including `other->_raw`
/// cross-instance ivar access and `[[OZQ31 alloc] init]` chaining -- both
/// confirmed to transpile and run correctly through oz_static). Requires
/// a root class in scope named `OZSRoot` with an `- (instancetype)init`
/// that returns `self` (see e.g. behavior_foundation_q31.rs's PREAMBLE).
/// Shared by every test file that needs OZQ31 so the ~250-line body isn't
/// duplicated per file.
pub const OZQ31_SRC: &str = "\
#include <stdint.h>

#ifndef _OZ_Q31_HELPERS
#define _OZ_Q31_HELPERS

static inline uint8_t _oz_bits_for_mag(uint32_t mag) {
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

static inline uint8_t _oz_shift_for_float(float value) {
	if (value == 0.0f) {
		return 0;
	}
	float mag = (value < 0.0f) ? -value : value;
	return _oz_bits_for_mag((uint32_t)mag);
}

static inline uint8_t _oz_shift_for_int32(int32_t value) {
	if (value == 0) {
		return 0;
	}
	uint32_t mag = (value < 0) ? (uint32_t)(-(int64_t)value) : (uint32_t)value;
	return _oz_bits_for_mag(mag);
}

static inline int32_t _oz_encode_float(float value, uint8_t shift) {
	if (shift >= 31) {
		return (int32_t)(value * 0.5f);
	}
	return (int32_t)(value * (float)(1UL << (31 - shift)));
}

static inline int32_t _oz_encode_int32(int32_t value, uint8_t shift) {
	return value << (31 - shift);
}

static inline float _oz_decode_float(int32_t raw, uint8_t shift) {
	if (shift >= 31) {
		return (float)raw;
	}
	return (float)raw / (float)(1UL << (31 - shift));
}

static inline int32_t _oz_decode_int32(int32_t raw, uint8_t shift) {
	if (shift >= 31) {
		return raw;
	}
	return raw >> (31 - shift);
}

static inline void _oz_align_shift(int32_t *raw_a, uint8_t shift_a, int32_t *raw_b,
				    uint8_t shift_b, uint8_t *out_shift) {
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
 * No stdio, no float -- pure integer math. Trailing zero removal.
 * precision: number of fractional digits (clamped to 0..14).
 */
static inline int _oz_q31_to_str(int32_t raw, uint8_t shift, char *buf, int maxLen,
				  int precision) {
	if (maxLen <= 0) {
		return 0;
	}

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

	int last_frac = -1;
	for (int i = precision - 1; i >= 0; i--) {
		if (frac_digits[i] != 0) {
			last_frac = i;
			break;
		}
	}

	if (neg && pos < maxLen) {
		buf[pos++] = '-';
	}

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
 * No float decode/encode -- works entirely in Q31 domain.
 */
static inline void _oz_q31_div(int32_t a_raw, uint8_t a_shift, int32_t b_raw, uint8_t b_shift,
				int32_t *out_raw, uint8_t *out_shift) {
	if (b_raw == 0) {
		*out_raw = 0;
		*out_shift = 0;
		return;
	}

	int neg = ((a_raw ^ b_raw) < 0) ? 1 : 0;
	uint64_t a = (a_raw < 0) ? (uint64_t)(-(int64_t)a_raw) : (uint64_t)a_raw;
	uint64_t b = (b_raw < 0) ? (uint64_t)(-(int64_t)b_raw) : (uint64_t)b_raw;

	uint64_t a_norm = a << a_shift;
	uint64_t b_norm = b << b_shift;

	uint64_t q_int = a_norm / b_norm;
	uint64_t q_rem = a_norm % b_norm;

	if (q_int > (uint64_t)INT32_MAX) {
		*out_raw = neg ? INT32_MIN : INT32_MAX;
		*out_shift = 31;
		return;
	}

	uint8_t result_shift = _oz_bits_for_mag((uint32_t)q_int);
	uint8_t frac_bits = 31 - result_shift;

	uint64_t result = q_int << frac_bits;

	for (int i = (int)frac_bits - 1; i >= 0; i--) {
		q_rem <<= 1;
		if (q_rem >= b_norm) {
			result |= ((uint64_t)1 << i);
			q_rem -= b_norm;
		}
	}

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

@interface OZQ31 : OZSRoot {
	int32_t _raw;
	uint8_t _shift;
}
+ (instancetype)fixedWithFloat:(float)value;
+ (instancetype)fixedWithInt32:(int32_t)value;
+ (instancetype)fixedWithRaw:(int32_t)raw shift:(uint8_t)shift;

- (int8_t)int8Value;
- (uint16_t)uint16Value;
- (int32_t)int32Value;
- (float)floatValue;
- (int)boolValue;
- (int)intValue;

- (int32_t)rawValue;
- (uint8_t)shift;

- (instancetype)add:(OZQ31 *)other;
- (instancetype)sub:(OZQ31 *)other;
- (instancetype)mul:(OZQ31 *)other;
- (instancetype)div:(OZQ31 *)other;
@end

@implementation OZQ31

+ (instancetype)fixedWithFloat:(float)value {
	OZQ31 *fp = [[OZQ31 alloc] init];
	fp->_shift = _oz_shift_for_float(value);
	fp->_raw = _oz_encode_float(value, fp->_shift);
	return fp;
}

+ (instancetype)fixedWithInt32:(int32_t)value {
	OZQ31 *fp = [[OZQ31 alloc] init];
	fp->_shift = _oz_shift_for_int32(value);
	fp->_raw = _oz_encode_int32(value, fp->_shift);
	return fp;
}

+ (instancetype)fixedWithRaw:(int32_t)raw shift:(uint8_t)shift {
	OZQ31 *fp = [[OZQ31 alloc] init];
	fp->_raw = raw;
	fp->_shift = shift;
	return fp;
}

- (int8_t)int8Value {
	return (int8_t)_oz_decode_int32(_raw, _shift);
}

- (uint16_t)uint16Value {
	return (uint16_t)_oz_decode_int32(_raw, _shift);
}

- (int32_t)int32Value {
	return _oz_decode_int32(_raw, _shift);
}

- (float)floatValue {
	return _oz_decode_float(_raw, _shift);
}

- (int)boolValue {
	return _raw != 0;
}

- (int)intValue {
	return (int)_oz_decode_int32(_raw, _shift);
}

- (int32_t)rawValue {
	return _raw;
}

- (uint8_t)shift {
	return _shift;
}

- (instancetype)add:(OZQ31 *)other {
	int32_t a = _raw;
	int32_t b = other->_raw;
	uint8_t s;
	_oz_align_shift(&a, _shift, &b, other->_shift, &s);

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

- (instancetype)sub:(OZQ31 *)other {
	int32_t a = _raw;
	int32_t b = other->_raw;
	uint8_t s;
	_oz_align_shift(&a, _shift, &b, other->_shift, &s);

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

- (instancetype)mul:(OZQ31 *)other {
	int64_t product = (int64_t)_raw * (int64_t)other->_raw;
	int32_t result_raw = (int32_t)(product >> 31);
	uint8_t result_shift = _shift + other->_shift;
	if (result_shift > 31) {
		result_shift = 31;
	}
	return [OZQ31 fixedWithRaw:result_raw shift:result_shift];
}

- (instancetype)div:(OZQ31 *)other {
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(_raw, _shift, other->_raw, other->_shift, &r_raw, &r_shift);
	return [OZQ31 fixedWithRaw:r_raw shift:r_shift];
}

@end
";
