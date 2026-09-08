/* Behavior test: a +1 result bound through a cast is released (#332).
 *
 * No entry point can assert anything about a leaked reference directly --
 * it has a name but no observable state, and a leak passes every
 * assertion about a return value. Each one reports what the slab says
 * instead: with the reference accounted for, a later allocation still
 * finds a free slot; with it leaked or double-retained, the two-slot slab
 * is full and `[Thing alloc]` answers nil.
 *
 * `test_init_bound_through_a_cast_is_left_alone` is the double-free
 * direction and is deliberately weak here: an over-release of one object
 * under two names is invisible to both a slot count and a dealloc
 * counter, and shows up only as a read of freed memory under
 * `--sanitize=address`. The `.m` says why, and names the Rust test that
 * gates it on every host.
 */
#include "unity.h"
#include "Thing_ozh.h"

int run_bound_through_cast(void);
int run_init_bound_through_cast(void);
int run_reassign_through_cast(void);
int run_ivar_store_through_cast(void);
int run_return_through_cast(void);

void test_owning_result_bound_through_a_cast_is_released(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_bound_through_cast());
}

/* The double-free guard: a cast must not hide that `-init...` hands back
 * its receiver's own reference. */
void test_init_bound_through_a_cast_is_left_alone(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_init_bound_through_cast());
}

void test_strong_local_reassigned_through_a_cast_releases_the_old_value(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_reassign_through_cast());
}

void test_strong_ivar_stored_through_a_cast_is_not_retained(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_ivar_store_through_cast());
}

void test_returning_through_a_cast_hands_the_caller_the_reference(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_return_through_cast());
}
