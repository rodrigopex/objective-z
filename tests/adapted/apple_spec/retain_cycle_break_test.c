/*
 * Behavioral spec derived from: Apple ARC documentation
 * Verifies a parent/child pair that would cycle is broken by an
 * `__unsafe_unretained` back-reference, and that both objects are freed.
 */
#include "unity.h"
#include "CycleTest_ozh.h"

void test_retain_cycle_is_broken(void)
{
	struct CycleTest *t = CycleTest_alloc();
	CycleTest_run(t);

	/* The pair is linked, not merely co-resident: `bTag` is read
	 * through A's strong ivar, and B knows its owner. */
	TEST_ASSERT_EQUAL_INT(1, CycleTest_aTag(t));
	TEST_ASSERT_EQUAL_INT(2, CycleTest_bTag(t));
	TEST_ASSERT_EQUAL_INT(1, CycleTest_linked(t));

	/* The control: one slot per pool, so a second alloc while the
	 * first is alive must be refused. Without it the next assertion
	 * would pass on a wider pool with nothing released (#455). */
	TEST_ASSERT_EQUAL_INT(1, CycleTest_poolIsOne(t));

	/* The claim: both pools recycled, so neither object outlived the
	 * scope. A strong back-reference would have kept both alive and
	 * this would be 0. */
	TEST_ASSERT_EQUAL_INT(1, CycleTest_brokeOk(t));

	OZObject_release((struct OZObject *)t);
}
