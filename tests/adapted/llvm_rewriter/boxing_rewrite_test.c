/*
 * Adapted from: clang/test/Rewriter/objc-modern-boxing.mm
 * Verifies @(expr) lowering produces correct OZQ31 factory calls.
 */
#include "unity.h"
#include "BoxingObj_ozh.h"
#include "OZQ31_ozh.h"

static inline int32_t fp_int32(struct OZQ31 *n)
{
	if (n->_shift >= 31) {
		return n->_raw;
	}
	return n->_raw >> (31 - n->_shift);
}

void test_boxing_literal(void)
{
	struct BoxingObj *b = BoxingObj_alloc();
	BoxingObj_run(b);
	struct OZQ31 *n = BoxingObj_literal(b);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(42, fp_int32(n));
	OZObject_release((struct OZObject *)b);
}

void test_boxing_expression(void)
{
	struct BoxingObj *b = BoxingObj_alloc();
	BoxingObj_run(b);
	struct OZQ31 *n = BoxingObj_expr(b);
	TEST_ASSERT_NOT_NULL(n);
	TEST_ASSERT_EQUAL_INT32(15, fp_int32(n));
	OZObject_release((struct OZObject *)b);
}
