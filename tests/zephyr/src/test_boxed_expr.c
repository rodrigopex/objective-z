/* SPDX-License-Identifier: Apache-2.0 */
/* Boxed expression tests: @(variable), @(arithmetic), @(call), @(float), @(uint) */
#include <zephyr/ztest.h>
#include "BoxedTest_ozh.h"
#include "OZNumber_ozh.h"
#include "OZObject_ozh.h"

ZTEST_SUITE(boxed_expr, NULL, NULL, NULL, NULL, NULL);

ZTEST(boxed_expr, test_boxed_variable_int)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromVar(bt);
	zassert_not_null(n, "fromVar returned NULL");
	zassert_equal(OZ_NUM_INT32, n->_tag, "Expected INT32 tag");
	zassert_equal(7, n->_value.i32, "Expected 7, got %d", n->_value.i32);
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_arithmetic)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromExpr(bt);
	zassert_not_null(n, "fromExpr returned NULL");
	zassert_equal(OZ_NUM_INT32, n->_tag, "Expected INT32 tag");
	zassert_equal(10, n->_value.i32, "Expected 10, got %d", n->_value.i32);
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_function_call)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromCall(bt);
	zassert_not_null(n, "fromCall returned NULL");
	zassert_equal(OZ_NUM_INT32, n->_tag, "Expected INT32 tag");
	zassert_equal(21, n->_value.i32, "Expected 21, got %d", n->_value.i32);
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_float)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromFloat(bt);
	zassert_not_null(n, "fromFloat returned NULL");
	zassert_equal(OZ_NUM_FLOAT, n->_tag, "Expected FLOAT tag");
	zassert_true(n->_value.f32 > 2.4f && n->_value.f32 < 2.6f,
		     "Expected ~2.5f");
	OZObject_release((struct OZObject *)bt);
}

ZTEST(boxed_expr, test_boxed_unsigned_int)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	zassert_not_null(bt, "BoxedTest alloc returned NULL");
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromUint(bt);
	zassert_not_null(n, "fromUint returned NULL");
	zassert_equal(OZ_NUM_UINT32, n->_tag, "Expected UINT32 tag");
	zassert_equal(1000u, n->_value.u32, "Expected 1000, got %u",
		      n->_value.u32);
	OZObject_release((struct OZObject *)bt);
}
