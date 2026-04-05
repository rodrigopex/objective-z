/* Behavior test: @synchronized(self) executes body and releases lock */
#include "unity.h"
#include "LockTest_ozh.h"

void test_synchronized_body_executes(void)
{
	struct LockTest *t = LockTest_alloc();
	TEST_ASSERT_NOT_NULL(t);

	LockTest_run(t);
	TEST_ASSERT_EQUAL_INT(42, LockTest_flag(t));

	OZObject_release((struct OZObject *)t);
}
