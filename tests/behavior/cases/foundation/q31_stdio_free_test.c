/*
 * Behavior test: OZQ31 integer-only cDescription and division.
 * Tests _oz_q31_to_str (spec, boundary, coverage) and _oz_q31_div via ObjC.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "Q31NoStdio_ozh.h"
#include "OZQ31_ozh.h"
#include <string.h>
#include <limits.h>

/* ── Helper: call _oz_q31_to_str and null-terminate ─────────────── */

static int q31_str(int32_t raw, uint8_t shift, char *buf, int maxLen)
{
	int n = _oz_q31_to_str(raw, shift, buf, maxLen);
	if (n < maxLen) {
		buf[n] = '\0';
	}
	return n;
}

/* ── _oz_q31_to_str: Specification-based tests ──────────────────── */

void test_str_zero(void)
{
	char buf[32];
	int n = q31_str(0, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_INT(1, n);
	TEST_ASSERT_EQUAL_STRING("0", buf);
}

void test_str_zero_any_shift(void)
{
	char buf[32];
	/* raw=0 regardless of shift */
	q31_str(0, 15, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0", buf);
}

void test_str_positive_int_one(void)
{
	/* @(1): shift=1, raw = 1 << 30 = 1073741824 */
	char buf[32];
	q31_str(1 << 30, 1, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("1", buf);
}

void test_str_positive_int_ten(void)
{
	/* @(10): shift=4, raw = 10 << 27 = 1342177280 */
	char buf[32];
	q31_str(10 << 27, 4, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("10", buf);
}

void test_str_positive_int_hundred(void)
{
	/* @(100): shift=7, raw = 100 << 24 = 1677721600 */
	char buf[32];
	q31_str(100 << 24, 7, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("100", buf);
}

void test_str_positive_int_thousand(void)
{
	/* @(1000): shift=10, raw = 1000 << 21 */
	char buf[32];
	q31_str(1000 << 21, 10, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("1000", buf);
}

void test_str_negative_int(void)
{
	/* @(-1): shift=1, raw = -(1 << 30) */
	char buf[32];
	q31_str(-(1 << 30), 1, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("-1", buf);
}

void test_str_negative_hundred(void)
{
	/* @(-100): shift=7, raw = -(100 << 24) */
	char buf[32];
	q31_str(-(100 << 24), 7, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("-100", buf);
}

void test_str_half(void)
{
	/* 0.5 in Q31 with shift=0: raw = 0.5 * 2^31 = 1073741824 */
	char buf[32];
	q31_str(1073741824, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0.5", buf);
}

void test_str_quarter(void)
{
	/* 0.25 in Q31 shift=0: raw = 0.25 * 2^31 = 536870912 */
	char buf[32];
	q31_str(536870912, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0.25", buf);
}

void test_str_three_and_half(void)
{
	/* 3.5: shift=2, raw = 3.5 * 2^29 = 1879048192 */
	char buf[32];
	q31_str(1879048192, 2, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("3.5", buf);
}

void test_str_trailing_zero_removal(void)
{
	/* 10.5: shift=4, raw = 10.5 * 2^27 = 1409286144 */
	char buf[32];
	q31_str(1409286144, 4, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("10.5", buf);
}

void test_str_six_decimal_places(void)
{
	/*
	 * 1/3 ≈ 0.333333...
	 * Q31 shift=0: raw = floor(1/3 * 2^31) = 715827882
	 * Expected: "0.333333" (6 decimal places, trailing zeros trimmed)
	 */
	char buf[32];
	q31_str(715827882, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0.333333", buf);
}

void test_str_rounding_up(void)
{
	/*
	 * 2/3 ≈ 0.666666...7
	 * Q31 shift=0: raw = floor(2/3 * 2^31) = 1431655765
	 * The 7th digit should be >= 5, causing rounding of 6th digit.
	 * Expected: "0.666667"
	 */
	char buf[32];
	q31_str(1431655765, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0.666667", buf);
}

void test_str_negative_fraction(void)
{
	/* -0.5: shift=0, raw = -1073741824 */
	char buf[32];
	q31_str(-1073741824, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("-0.5", buf);
}

/* ── _oz_q31_to_str: Boundary tests ────────────────────────────── */

void test_str_maxlen_zero(void)
{
	char buf[4] = "XYZ";
	int n = _oz_q31_to_str(1 << 30, 1, buf, 0);
	TEST_ASSERT_EQUAL_INT(0, n);
	/* buf untouched */
	TEST_ASSERT_EQUAL_CHAR('X', buf[0]);
}

void test_str_maxlen_one(void)
{
	/* Only first char fits */
	char buf[4] = {0};
	int n = q31_str(10 << 27, 4, buf, 1);
	TEST_ASSERT_EQUAL_INT(1, n);
	TEST_ASSERT_EQUAL_CHAR('1', buf[0]);
}

void test_str_maxlen_truncates_integer(void)
{
	/* "100" needs 3 chars; maxLen=2 gives "10" */
	char buf[4] = {0};
	int n = q31_str(100 << 24, 7, buf, 2);
	TEST_ASSERT_EQUAL_INT(2, n);
	TEST_ASSERT_EQUAL_CHAR('1', buf[0]);
	TEST_ASSERT_EQUAL_CHAR('0', buf[1]);
}

void test_str_shift_31_pure_integer(void)
{
	/* shift=31: frac_bits=0, value = raw directly */
	char buf[32];
	q31_str(42, 31, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("42", buf);
}

void test_str_shift_31_negative(void)
{
	char buf[32];
	q31_str(-99, 31, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("-99", buf);
}

void test_str_shift_0_smallest_positive(void)
{
	/* shift=0: smallest positive = 1/2^31 ≈ 0.000000000465 → "0" (rounds to zero in 6 places) */
	char buf[32];
	q31_str(1, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0", buf);
}

void test_str_rounding_carry_into_integer(void)
{
	/*
	 * Value very close to 1.0 from below.
	 * shift=0, raw = 2^31 - 1 = 2147483647 → value ≈ 0.999999999534
	 * 6 decimal places: 0.999999 → 7th digit is 9 → rounds up → 1.000000 → "1"
	 */
	char buf[32];
	q31_str(2147483647, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("1", buf);
}

void test_str_large_value_shift_31(void)
{
	/* shift=31: raw = 1000000 → "1000000" */
	char buf[32];
	q31_str(1000000, 31, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("1000000", buf);
}

/* ── _oz_q31_div: via ObjC interface ────────────────────────────── */

void test_div_ten_by_four(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 2.5f, Q31NoStdio_divTenByFour(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_ten_by_three(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 3.333f, Q31NoStdio_divTenByThree(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_neg_ten_by_two(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, -5.0f, Q31NoStdio_divNegTenByTwo(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_ten_by_neg_two(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, -5.0f, Q31NoStdio_divTenByNegTwo(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_neg_by_neg(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 5.0f, Q31NoStdio_divNegByNeg(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_self_by_self(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 1.0f, Q31NoStdio_divSelfBySelf(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_small_by_large(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.001f, 0.001f, Q31NoStdio_divSmallByLarge(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_large_by_small(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(1.0f, 1000.0f, Q31NoStdio_divLargeBySmall(t));
	OZObject_release((struct OZObject *)t);
}

void test_div_by_zero(void)
{
	struct Q31NoStdio *t = Q31NoStdio_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(0, Q31NoStdio_divByZeroRaw(t));
	OZObject_release((struct OZObject *)t);
}

/* ── _oz_q31_div: direct helper tests ──────────────────────────── */

void test_div_helper_exact(void)
{
	/* 10 / 2 = 5; value 2: shift=2, raw = 2 << 29 */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(10 << 27, 4, 2 << 29, 2, &r_raw, &r_shift);
	/* Decode result: r_raw >> (31 - r_shift) should be 5 */
	int32_t int_val = (r_shift >= 31) ? r_raw : (r_raw >> (31 - r_shift));
	TEST_ASSERT_EQUAL_INT(5, int_val);
}

void test_div_helper_zero_numerator(void)
{
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(0, 0, 1 << 30, 1, &r_raw, &r_shift);
	TEST_ASSERT_EQUAL_INT(0, r_raw);
}

void test_div_helper_zero_denominator(void)
{
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(10 << 27, 4, 0, 0, &r_raw, &r_shift);
	TEST_ASSERT_EQUAL_INT(0, r_raw);
	TEST_ASSERT_EQUAL_INT(0, r_shift);
}

void test_div_helper_identity(void)
{
	/* x / x = 1 for any x */
	int32_t r_raw;
	uint8_t r_shift;
	int32_t val = 42 << 25; /* shift=6 */
	_oz_q31_div(val, 6, val, 6, &r_raw, &r_shift);
	float result = (r_shift >= 31) ? (float)r_raw
		: (float)r_raw / (float)(1UL << (31 - r_shift));
	TEST_ASSERT_FLOAT_WITHIN(0.001f, 1.0f, result);
}

void test_div_helper_negative_result(void)
{
	/* 10 / -2 = -5; value -2: shift=2, raw = -(2 << 29) */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(10 << 27, 4, -(2 << 29), 2, &r_raw, &r_shift);
	int32_t int_val = (r_shift >= 31) ? r_raw : (r_raw >> (31 - r_shift));
	TEST_ASSERT_EQUAL_INT(-5, int_val);
}

/* ── _oz_q31_to_str + _oz_q31_div integration ──────────────────── */

void test_str_of_division_result(void)
{
	/* 10 / 4 = 2.5; value 4: shift=3, raw = 4 << 28 */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(10 << 27, 4, 4 << 28, 3, &r_raw, &r_shift);

	char buf[32];
	q31_str(r_raw, r_shift, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("2.5", buf);
}

void test_str_of_fractional_division(void)
{
	/* 1 / 4 = 0.25; value 4: shift=3, raw = 4 << 28 */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(1 << 30, 1, 4 << 28, 3, &r_raw, &r_shift);

	char buf[32];
	q31_str(r_raw, r_shift, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0.25", buf);
}

/* ── Additional boundary tests ──────────────────────────────────── */

void test_str_negative_with_fraction(void)
{
	/* -3.5: shift=2, raw = -(3.5 * 2^29) = -1879048192 */
	char buf[32];
	q31_str(-1879048192, 2, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("-3.5", buf);
}

void test_str_maxlen_truncates_sign(void)
{
	/* "-1" needs 2 chars; maxLen=1 gives just '-' */
	char buf[4] = {0};
	int n = q31_str(-(1 << 30), 1, buf, 1);
	TEST_ASSERT_EQUAL_INT(1, n);
	TEST_ASSERT_EQUAL_CHAR('-', buf[0]);
}

void test_str_maxlen_truncates_fraction(void)
{
	/* "0.5" needs 3 chars; maxLen=2 gives "0." */
	char buf[4] = {0};
	int n = q31_str(1073741824, 0, buf, 2);
	TEST_ASSERT_EQUAL_INT(2, n);
	TEST_ASSERT_EQUAL_CHAR('0', buf[0]);
	TEST_ASSERT_EQUAL_CHAR('.', buf[1]);
}

void test_str_one_eighth(void)
{
	/* 0.125: shift=0, raw = 0.125 * 2^31 = 268435456 */
	char buf[32];
	q31_str(268435456, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("0.125", buf);
}

void test_str_shift_1_value_one(void)
{
	/* Verify shift=1 encodes value 1 correctly */
	char buf[32];
	q31_str(1 << 30, 1, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("1", buf);
}

void test_str_shift_30(void)
{
	/* shift=30: frac_bits=1, can represent .0 or .5 */
	/* raw = 3 << 1 = 6 (but in Q1 format), value = 6 / 2 = 3 */
	/* Actually: shift=30, raw=3, value = 3 / 2^1 = 1.5 */
	char buf[32];
	q31_str(3, 30, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("1.5", buf);
}

void test_div_helper_asymmetric_shifts(void)
{
	/* a=1000 (shift=10), b=10 (shift=4) → 100 */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(1000 << 21, 10, 10 << 27, 4, &r_raw, &r_shift);
	int32_t int_val = (r_shift >= 31) ? r_raw : (r_raw >> (31 - r_shift));
	TEST_ASSERT_EQUAL_INT(100, int_val);
}

void test_div_helper_both_negative(void)
{
	/* -10 / -2 = 5 */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(-(10 << 27), 4, -(2 << 29), 2, &r_raw, &r_shift);
	int32_t int_val = (r_shift >= 31) ? r_raw : (r_raw >> (31 - r_shift));
	TEST_ASSERT_EQUAL_INT(5, int_val);
}

void test_div_helper_result_less_than_one(void)
{
	/* 1 / 3 ≈ 0.333... */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(1 << 30, 1, 3 << 29, 2, &r_raw, &r_shift);
	float result = (r_shift >= 31) ? (float)r_raw
		: (float)r_raw / (float)(1UL << (31 - r_shift));
	TEST_ASSERT_FLOAT_WITHIN(0.001f, 0.333f, result);
}

void test_str_all_nines_rounding(void)
{
	/*
	 * 0.9999995 in Q31 shift=0: raw = floor(0.9999995 * 2^31) = 2147483537
	 * 6 decimal places → 0.999999 → 7th digit 5 → rounds up → 1.000000 → "1"
	 */
	char buf[32];
	q31_str(2147483537, 0, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("1", buf);
}

void test_str_exact_power_of_two(void)
{
	/* 256: shift=9, raw = 256 << 22 = 1073741824 */
	char buf[32];
	q31_str(256 << 22, 9, buf, sizeof(buf));
	TEST_ASSERT_EQUAL_STRING("256", buf);
}

void test_div_helper_large_by_one(void)
{
	/* 500 / 1 = 500 */
	int32_t r_raw;
	uint8_t r_shift;
	_oz_q31_div(500 << 22, 9, 1 << 30, 1, &r_raw, &r_shift);
	int32_t int_val = (r_shift >= 31) ? r_raw : (r_raw >> (31 - r_shift));
	TEST_ASSERT_EQUAL_INT(500, int_val);
}
