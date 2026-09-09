/* Behavior test: `self->_ivar = value` retains like `_ivar = value` (#352).
 *
 * See the `.m` for why each direction needs its own observable -- in
 * short, a premature free leaves *more* free slots, so only reading the
 * ivar back after allocating into the freed block detects it, while an
 * over-release shows up as a slab that has lost count.
 */
#include "unity.h"
#include "Thing_ozh.h"

int run_stored_ivar_survives(void);
int run_repeated_store_balances(void);

void test_ivar_stored_through_self_outlives_the_storing_method(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_stored_ivar_survives());
}

void test_repeated_stores_through_self_release_exactly_the_old_value(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_repeated_store_balances());
}
