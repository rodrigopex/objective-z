/* Behavior test: @(expr) boxed expressions with OZFixedPoint */
#include "unity.h"
#include "BoxedTest_ozh.h"
#include "OZFixedPoint_ozh.h"

/* Decode Q31+shift to int32: raw >> (31 - shift) */
static inline int32_t fp_int32(struct OZFixedPoint *n)
{
	if (n->_shift >= 31) {
		return n->_raw;
	}
	return n->_raw >> (31 - n->_shift);
}

/* Decode Q31+shift to float: raw / 2^(31-shift) */
static inline float fp_float(struct OZFixedPoint *n)
{
	if (n->_shift >= 31) {
		return (float)n->_raw;
	}
	return (float)n->_raw / (float)(1UL << (31 - n->_shift));
}

void test_boxed_variable_int(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromVar(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(7, fp_int32(n));
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_arithmetic(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromExpr(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(10, fp_int32(n));
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_function_call(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromCall(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(21, fp_int32(n));
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_float(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromFloat(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 2.5f, fp_float(n));
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_unsigned_int(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromUint(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_UINT32(1000, (uint32_t)fp_int32(n));
	OZObject_release((struct OZObject *)bt);
}
