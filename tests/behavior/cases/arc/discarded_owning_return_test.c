/* Behavior test: a discarded +1 result is released (#322).
 *
 * Neither entry point can assert anything about the abandoned reference
 * itself -- it has no name to check. Each reports whether a later
 * allocation still found a free slab slot, which is 1 only if the
 * discarded `-copy` gave its slot back.
 */
#include "unity.h"
#include "Thing_ozh.h"

int run_discarded_direct(void);
int run_discarded_via_selector(void);

void test_discarded_owning_return_direct(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_discarded_direct());
}

void test_discarded_owning_return_via_selector(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_discarded_via_selector());
}
