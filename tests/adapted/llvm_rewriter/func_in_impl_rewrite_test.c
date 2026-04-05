/*
 * Adapted from: clang/test/Rewriter/func-in-impl.m
 * Verifies C functions defined near @implementation are preserved.
 */
#include "unity.h"
#include "FuncInImpl_ozh.h"

void test_func_in_impl_preserved(void)
{
	struct FuncInImpl *f = FuncInImpl_alloc();
	FuncInImpl_run(f);
	TEST_ASSERT_EQUAL_INT(36, FuncInImpl_val(f));
	OZObject_release((struct OZObject *)f);
}
