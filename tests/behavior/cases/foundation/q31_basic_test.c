/* Behavior test: OZQ31 Q31+shift encoding, extraction, arithmetic */
#include "unity.h"
#include "oz_dispatch.h"
#include "FPTest_ozh.h"

/* ── Value extraction roundtrip ──────────────────────────────────── */

void test_int_from_literal(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(42, FPTest_intFromLiteral(t));
	OZObject_release((struct OZObject *)t);
}

void test_float_from_literal(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.1f, 3.5f, FPTest_floatFromLiteral(t));
	OZObject_release((struct OZObject *)t);
}

void test_int_from_expr(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(10, FPTest_intFromExpr(t));
	OZObject_release((struct OZObject *)t);
}

void test_int8_roundtrip(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(100, FPTest_int8Roundtrip(t));
	OZObject_release((struct OZObject *)t);
}

void test_uint16_roundtrip(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(1000, FPTest_uint16Roundtrip(t));
	OZObject_release((struct OZObject *)t);
}

void test_bool_true(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(1, FPTest_boolTrue(t));
	OZObject_release((struct OZObject *)t);
}

void test_bool_false(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(0, FPTest_boolFalse(t));
	OZObject_release((struct OZObject *)t);
}

/* ── Q31 introspection ───────────────────────────────────────────── */

void test_raw_nonzero(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(1, FPTest_rawNonZero(t));
	OZObject_release((struct OZObject *)t);
}

void test_shift_for_ten(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	/* 10 needs 4 integer bits (2^4 = 16 > 10), so shift = 4 */
	TEST_ASSERT_EQUAL_INT(4, FPTest_shiftForTen(t));
	OZObject_release((struct OZObject *)t);
}

/* ── Arithmetic ──────────────────────────────────────────────────── */

void test_add(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(30, FPTest_addResult(t));
	OZObject_release((struct OZObject *)t);
}

void test_sub(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(30, FPTest_subResult(t));
	OZObject_release((struct OZObject *)t);
}

void test_mul(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(42, FPTest_mulResult(t));
	OZObject_release((struct OZObject *)t);
}

void test_div(void)
{
	struct FPTest *t = FPTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_FLOAT_WITHIN(0.1f, 2.5f, FPTest_divResult(t));
	OZObject_release((struct OZObject *)t);
}
