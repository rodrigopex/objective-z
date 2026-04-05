/* Behavior test: reassigning strong local releases previous object */
#include "unity.h"
#include "ArcReassignTest_ozh.h"

void test_reassign_releases_old_object(void)
{
	struct ArcReassignTest *t = ArcReassignTest_alloc();
	ArcReassignTest_run(t);
	/* canRealloc=1 means slab was freed during reassignment */
	TEST_ASSERT_EQUAL_INT(1, ArcReassignTest_canRealloc(t));
	OZObject_release((struct OZObject *)t);
}
