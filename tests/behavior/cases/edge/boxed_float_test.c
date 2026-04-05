/* Behavior test: @(float_var) boxes via OZQ31 float path */
#include "unity.h"
#include "BoxedFloatTest_ozh.h"
#include "OZQ31_ozh.h"

static inline float fp_float(struct OZQ31 *n)
{
	if (n->_shift >= 31) {
		return (float)n->_raw;
	}
	return (float)n->_raw / (float)(1UL << (31 - n->_shift));
}

void test_boxed_float_value(void)
{
	struct BoxedFloatTest *t = BoxedFloatTest_alloc();
	BoxedFloatTest_run(t);
	struct OZQ31 *n = BoxedFloatTest_boxed(t);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_FLOAT_WITHIN(0.01f, 3.14f, fp_float(n));
	OZObject_release((struct OZObject *)t);
}
