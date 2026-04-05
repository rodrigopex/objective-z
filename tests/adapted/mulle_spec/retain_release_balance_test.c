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
	TEST_ASSERT_EQUAL_INT(1, BalanceTest_reuseOk(t));
	OZObject_release((struct OZObject *)t);
}
