/* Behavior test: @(enum_value) boxes via OZQ31 int32 path */
#include "unity.h"
#include "BoxedEnumTest_ozh.h"
#include "OZQ31_ozh.h"

static inline int32_t fp_int32(struct OZQ31 *n)
{
	if (n->_shift >= 31) {
		return n->_raw;
	}
	return n->_raw >> (31 - n->_shift);
}

void test_boxed_enum_ok(void)
{
	struct BoxedEnumTest *t = BoxedEnumTest_alloc();
	BoxedEnumTest_boxStatus_(t, 200);
	struct OZQ31 *n = BoxedEnumTest_boxed(t);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(200, fp_int32(n));
	OZObject_release((struct OZObject *)t);
}

void test_boxed_enum_not_found(void)
{
	struct BoxedEnumTest *t = BoxedEnumTest_alloc();
	BoxedEnumTest_boxStatus_(t, 404);
	struct OZQ31 *n = BoxedEnumTest_boxed(t);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(404, fp_int32(n));
	OZObject_release((struct OZObject *)t);
}
