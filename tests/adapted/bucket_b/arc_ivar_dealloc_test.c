/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 * Verifies object ivars are released when owner is deallocated.
 */
#include "unity.h"
#include "IvarDeallocTest_ozh.h"

void test_ivar_released_on_dealloc(void)
{
	struct IvarDeallocTest *t = IvarDeallocTest_alloc();
	IvarDeallocTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, IvarDeallocTest_canReallocChild(t));
	OZObject_release((struct OZObject *)t);
}
