/* Behavior test: OZDefer lifecycle — block fires on dealloc without crash */
#include "unity.h"
#include "oz_dispatch.h"
#include "DeferTest_ozh.h"

void test_defer_lifecycle_with_owner(void)
{
	struct DeferTest *t = DeferTest_alloc();
	TEST_ASSERT_NOT_NULL(t);
	DeferTest_initWithCleanup(t);
	TEST_ASSERT_EQUAL_INT(99, DeferTest_marker(t));
	/* Release triggers DeferTest_dealloc → releases _cleanup ivar
	 * → OZDefer_dealloc fires block with owner → no crash */
	OZObject_release((struct OZObject *)t);
}
