/* Behavior test: a +1 result passed as an argument is released (#328).
 *
 * No entry point can assert anything about a leaked reference directly --
 * a value passed as an argument has no name in the caller at all, and a
 * leak passes every assertion about a return value. Each one reports what
 * the slab says instead: with the argument's reference accounted for, a
 * later allocation still finds a free slot; with it leaked on top of the
 * setter's retain, the two-slot slab is full and `[Thing alloc]` answers
 * nil.
 *
 * `test_init_argument_is_left_alone` is the double-free direction and is
 * deliberately weak here: an over-release of one object under two names is
 * invisible to both a slot count and a dealloc counter, and shows up only
 * as a read of freed memory under `--sanitize=address`. The `.m` says why,
 * and names the Rust test that gates it on every host.
 */
#include "unity.h"
#include "Thing_ozh.h"

int run_owning_argument_to_setter(void);
int run_nested_alloc_init_argument(void);
int run_owning_argument_borrowed_by_callee(void);
int run_owning_argument_in_declaration(void);
int run_borrowed_argument_stays(void);
int run_init_argument_left_alone(void);

void test_owning_argument_to_a_setter_is_released(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_owning_argument_to_setter());
}

void test_nested_alloc_init_argument_is_released_once(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_nested_alloc_init_argument());
}

void test_owning_argument_borrowed_by_the_callee_is_released(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_owning_argument_borrowed_by_callee());
}

void test_owning_argument_in_a_declaration_is_released(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_owning_argument_in_declaration());
}

/* The guard that fails closed: a borrowed argument stays the caller's. */
void test_borrowed_argument_is_not_released_by_the_call_site(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_borrowed_argument_stays());
}

/* The double-free guard: an `-init...` argument hands back a reference its
 * receiver already accounts for. */
void test_init_argument_is_left_alone(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_init_argument_left_alone());
}
