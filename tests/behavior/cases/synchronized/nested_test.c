/* Behavior test: nested @synchronized with different objects */
#include "unity.h"
#include "NestLock_ozh.h"

void test_nested_synchronized_both_execute(void)
{
	struct NestLock *a = NestLock_alloc();
	struct NestLock *b = NestLock_alloc();
	TEST_ASSERT_NOT_NULL(a);
	TEST_ASSERT_NOT_NULL(b);

	NestLock_runNested_(a, b);
	TEST_ASSERT_EQUAL_INT(1, NestLock_outer(a));
	TEST_ASSERT_EQUAL_INT(2, NestLock_inner(a));

	OZObject_release((struct OZObject *)b);
	OZObject_release((struct OZObject *)a);
}
