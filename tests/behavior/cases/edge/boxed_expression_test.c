/* Behavior test: @(expr) boxed expressions */
#include "unity.h"
#include "BoxedTest_ozh.h"
#include "OZNumber_ozh.h"

void test_boxed_variable_int(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromVar(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT(OZ_NUM_INT32, n->_tag);
	TEST_ASSERT_EQUAL_INT32(7, n->_value.i32);
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_arithmetic(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromExpr(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(10, n->_value.i32);
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_function_call(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromCall(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(21, n->_value.i32);
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_float(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromFloat(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT(OZ_NUM_FLOAT, n->_tag);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 2.5f, n->_value.f32);
	OZObject_release((struct OZObject *)bt);
}

void test_boxed_unsigned_int(void)
{
	struct BoxedTest *bt = BoxedTest_alloc();
	BoxedTest_run(bt);
	struct OZNumber *n = BoxedTest_fromUint(bt);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT(OZ_NUM_UINT32, n->_tag);
	TEST_ASSERT_EQUAL_UINT32(1000, n->_value.u32);
	OZObject_release((struct OZObject *)bt);
}
