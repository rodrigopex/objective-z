/* Behavior test: break inside loop releases local object (slab reuse proves it) */
#include "unity.h"
#include "ArcBreakTest_ozh.h"

void test_break_releases_loop_local(void)
{
	struct ArcBreakTest *t = ArcBreakTest_alloc();
	ArcBreakTest_run(t);
	/* flag=1 means we could re-alloc from the 1-block slab after break */
	TEST_ASSERT_EQUAL_INT(1, ArcBreakTest_flag(t));
	OZObject_release((struct OZObject *)t);
}
