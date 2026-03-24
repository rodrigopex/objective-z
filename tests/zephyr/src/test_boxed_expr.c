/* SPDX-License-Identifier: Apache-2.0 */
/* Boxed expression tests: @(variable), @(arithmetic), @(call), @(float), @(uint) */
#include <zephyr/ztest.h>
#include "BoxedTest_ozh.h"
#include "OZQ31_ozh.h"
#include "OZObject_ozh.h"

/* Decode Q31+shift to int32: raw >> (31 - shift) */
static inline int32_t q31_int32(struct OZQ31 *n)
{
	if (n->_shift >= 31) {
		return n->_raw;
	}
	return n->_raw >> (31 - n->_shift);
}

/* Decode Q31+shift to float: raw / 2^(31-shift) */
static inline float q31_float(struct OZQ31 *n)
{
	if (n->_shift >= 31) {
		return (float)n->_raw;
	}
	return (float)n->_raw / (float)(1UL << (31 - n->_shift));
}

ZTEST_SUITE(boxed_expr, NULL, NULL, NULL, NULL, NULL);

ZTEST(boxed_expr, test_boxed_variable_int)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZQ31 *n = BoxedTest_fromVar(bt);
	zassert_not_null(n, "fromVar returned NULL");
	zassert_equal(7, q31_int32(n), "Expected 7");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_arithmetic)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZQ31 *n = BoxedTest_fromExpr(bt);
	zassert_not_null(n, "fromExpr returned NULL");
	zassert_equal(10, q31_int32(n), "Expected 10");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_function_call)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZQ31 *n = BoxedTest_fromCall(bt);
	zassert_not_null(n, "fromCall returned NULL");
	zassert_equal(21, q31_int32(n), "Expected 21");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_float)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZQ31 *n = BoxedTest_fromFloat(bt);
	zassert_not_null(n, "fromFloat returned NULL");
	float val = q31_float(n);
	zassert_true(val > 2.4f && val < 2.6f, "Expected ~2.5f");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_unsigned_int)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZQ31 *n = BoxedTest_fromUint(bt);
	zassert_not_null(n, "fromUint returned NULL");
	zassert_equal(1000u, (uint32_t)q31_int32(n), "Expected 1000");
	OZObject_release((struct OZObject *)bt);
}
