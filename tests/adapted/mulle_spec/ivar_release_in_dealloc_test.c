/*
 * Behavioral spec derived from: mulle-objc runtime lifecycle patterns
 * Verifies strong ivar is released when owner is deallocated.
 */
#include "unity.h"
#include "IvarRelTest_ozh.h"

void test_ivar_released_in_dealloc(void)
{
	struct IvarRelTest *t = IvarRelTest_alloc();
	IvarRelTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, IvarRelTest_canReallocOwned(t));
	OZObject_release((struct OZObject *)t);
}
