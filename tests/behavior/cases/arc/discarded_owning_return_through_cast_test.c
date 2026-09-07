/* Behavior test: a discarded +1 result reached through a cast is released
 * (#327).
 *
 * No entry point can assert anything about the abandoned reference
 * itself -- it has no name to check, and a leak passes every assertion
 * about a return value. Each one reports what the slab says instead: the
 * three discard cases are 1 only if the reference gave its slot back, and
 * the last is 1 only if it did *not*, since a discarded `-init` hands back
 * a reference its receiver -- a local ARC already releases -- still owns.
 */
#include "unity.h"
#include "Thing_ozh.h"

int run_discarded_through_void_cast(void);
int run_discarded_through_pointer_cast(void);
int run_discarded_init_behind_cast_receiver(void);
int run_init_through_cast_on_an_owned_local(void);

void test_discarded_owning_return_through_void_cast(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_discarded_through_void_cast());
}

void test_discarded_owning_return_through_pointer_cast(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_discarded_through_pointer_cast());
}

void test_discarded_init_behind_a_cast_receiver(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_discarded_init_behind_cast_receiver());
}

/* The double-free guard: a cast must not hide that a discarded `-init`
 * hands back its receiver's own reference. */
void test_init_through_a_cast_on_an_owned_local_is_left_alone(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_init_through_cast_on_an_owned_local());
}
