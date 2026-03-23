/* SPDX-License-Identifier: Apache-2.0 */
/* Boxed expression tests: @(variable), @(arithmetic), @(call), @(float), @(uint) */
#include <zephyr/ztest.h>
#include "BoxedTest_ozh.h"
#include "OZFixedPoint_ozh.h"
#include "OZObject_ozh.h"

ZTEST_SUITE(boxed_expr, NULL, NULL, NULL, NULL, NULL);

ZTEST(boxed_expr, test_boxed_variable_int)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromVar(bt);
	zassert_not_null(n, "fromVar returned NULL");
	zassert_equal(7, OZFixedPoint_int32Value(n), "Expected 7");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_arithmetic)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromExpr(bt);
	zassert_not_null(n, "fromExpr returned NULL");
	zassert_equal(10, OZFixedPoint_int32Value(n), "Expected 10");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_function_call)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromCall(bt);
	zassert_not_null(n, "fromCall returned NULL");
	zassert_equal(21, OZFixedPoint_int32Value(n), "Expected 21");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_float)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromFloat(bt);
	zassert_not_null(n, "fromFloat returned NULL");
	float val = OZFixedPoint_floatValue(n);
	zassert_true(val > 2.4f && val < 2.6f, "Expected ~2.5f");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_unsigned_int)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZFixedPoint *n = BoxedTest_fromUint(bt);
	zassert_not_null(n, "fromUint returned NULL");
	zassert_equal(1000u, OZFixedPoint_uint32Value(n), "Expected 1000");
	OZObject_release((struct OZObject *)bt);
}
