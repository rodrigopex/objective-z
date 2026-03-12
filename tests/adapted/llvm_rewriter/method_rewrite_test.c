/*
 * Adapted from: clang/test/Rewriter method rewriting
 * License: Apache 2.0 with LLVM Exception
 * Verifies multi-argument selector mangling into C function names.
 */
#include "unity.h"
#include "Calc_ozh.h"

void test_two_arg_selector(void)
{
	struct Calc *c = Calc_alloc();
	OZ_SEND_init((struct OZObject *)c);

	int result = Calc_add_to_(c, 3, 7);
	TEST_ASSERT_EQUAL_INT(10, result);

	OZObject_release((struct OZObject *)c);
}

void test_three_arg_selector(void)
{
	struct Calc *c = Calc_alloc();
	OZ_SEND_init((struct OZObject *)c);

	int result = Calc_multiply_by_offset_(c, 4, 5, 3);
	TEST_ASSERT_EQUAL_INT(23, result);

	OZObject_release((struct OZObject *)c);
}
