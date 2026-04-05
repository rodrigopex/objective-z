/* Behavior test: @synchronized protecting a shared counter */
#include "unity.h"
#include "SyncCounter_ozh.h"

void test_synchronized_counter_increments(void)
{
	struct SyncCounter *c = SyncCounter_alloc();
	TEST_ASSERT_NOT_NULL(c);

	SyncCounter_increment(c);
	SyncCounter_increment(c);
	SyncCounter_increment(c);
	TEST_ASSERT_EQUAL_INT(3, SyncCounter_count(c));

	OZObject_release((struct OZObject *)c);
}

void test_synchronized_counter_starts_at_zero(void)
{
	struct SyncCounter *c = SyncCounter_alloc();
	TEST_ASSERT_NOT_NULL(c);

	TEST_ASSERT_EQUAL_INT(0, SyncCounter_count(c));

	OZObject_release((struct OZObject *)c);
}
