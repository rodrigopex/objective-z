/* Behavior test: @(functionCall()) evaluates call and boxes result */
#include "unity.h"
#include "BoxedCallTest_ozh.h"
#include "OZQ31_ozh.h"

static inline int32_t fp_int32(struct OZQ31 *n)
{
	if (n->_shift >= 31) {
		return n->_raw;
	}
	return n->_raw >> (31 - n->_shift);
}

void test_boxed_call_expr(void)
{
	struct BoxedCallTest *t = BoxedCallTest_alloc();
	BoxedCallTest_run(t);
	struct OZQ31 *n = BoxedCallTest_boxed(t);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(99, fp_int32(n));
	OZObject_release((struct OZObject *)t);
}
