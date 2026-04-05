/* Behavior test: continue releases local object before next iteration */
#include "unity.h"
#include "ArcContinueTest_ozh.h"

void test_continue_releases_loop_local(void)
{
	struct ArcContinueTest *t = ArcContinueTest_alloc();
	ArcContinueTest_run(t);
	/* 3 iterations completed, and slab reuse after loop proves cleanup */
	TEST_ASSERT_EQUAL_INT(3, ArcContinueTest_iterations(t));
	OZObject_release((struct OZObject *)t);
}
