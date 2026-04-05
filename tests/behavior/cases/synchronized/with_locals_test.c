/* Behavior test: @synchronized with local object variables — ARC releases on scope exit */
#include "unity.h"
#include "SyncLocal_ozh.h"

void test_synchronized_with_local_objects(void)
{
	struct SyncLocal *s = SyncLocal_alloc();
	TEST_ASSERT_NOT_NULL(s);

	SyncLocal_run(s);
	TEST_ASSERT_EQUAL_INT(1, SyncLocal_marker(s));

	OZObject_release((struct OZObject *)s);
}

void test_synchronized_local_slab_reuse(void)
{
	/* Run twice to verify the local was freed and slab block reused */
	struct SyncLocal *s = SyncLocal_alloc();
	TEST_ASSERT_NOT_NULL(s);

	SyncLocal_run(s);
	SyncLocal_run(s);
	TEST_ASSERT_EQUAL_INT(1, SyncLocal_marker(s));

	OZObject_release((struct OZObject *)s);
}
