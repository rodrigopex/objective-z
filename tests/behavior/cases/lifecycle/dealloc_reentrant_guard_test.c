/* Behavior test: re-entrant dealloc guard via _meta.deallocating bit.
 * A dealloc that retains+releases self must not recurse infinitely. */
#include "unity.h"
#include "Probe_ozh.h"

void test_reentrant_dealloc_no_recursion(void)
{
	struct Probe *p = Probe_alloc();
	TEST_ASSERT_NOT_NULL(p);

	/* Release drops rc 1->0, triggers dealloc.
	 * Inside dealloc: retain (rc 0->1), release (rc 1->0),
	 * dec_and_test returns true but deallocating=1 -> returns.
	 * If the guard is missing, this would recurse infinitely. */
	OZObject_release((struct OZObject *)p);

	/* If we reach here, the guard works */
	TEST_PASS();
}
