/*
 * Behavioral spec derived from: mulle-objc runtime lifecycle patterns
 * Verifies object lifecycle: alloc succeeds, scope-exit frees slab for reuse.
 */
#include "unity.h"
#include "BalanceTest_ozh.h"

void test_retain_release_balance(void)
{
	struct BalanceTest *t = BalanceTest_alloc();
	BalanceTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, BalanceTest_allocOk(t));
	/*
	 * The control, and the assertion that makes the next one mean
	 * something: a full one-slot pool must refuse a second alloc. If
	 * this fails the pool is not one slot, and `reuseOk` below would
	 * pass whether or not anything was ever released (#455).
	 */
	TEST_ASSERT_EQUAL_INT(1, BalanceTest_poolIsOne(t));
	/* Re-alloc after scope exit: only possible if ARC released. */
	TEST_ASSERT_EQUAL_INT(1, BalanceTest_reuseOk(t));
	OZObject_release((struct OZObject *)t);
}
