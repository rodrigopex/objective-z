/* Behavior test: a `return` must not release the reference it hands back
 * (#351).
 *
 * Four assertions, two shapes by two failure directions. The `.m` explains
 * why each direction needs its own observable -- in short, a premature
 * free leaves *more* free slots, so only re-allocating into the freed
 * block detects it, while a leak is the only one a slot count can see.
 */
#include "unity.h"
#include "Thing_ozh.h"

int run_alias_return_is_live(void);
int run_opaque_call_return_is_live(void);
int run_alias_return_is_not_leaked(void);
int run_opaque_call_return_is_not_leaked(void);

/* The returned reference outlives the function that made it: a tag written
 * through the returned name must not be visible through a later,
 * unrelated allocation. */
void test_returned_alias_is_not_freed_on_the_way_out(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_alias_return_is_live());
}

void test_returned_call_result_is_not_freed_on_the_way_out(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_opaque_call_return_is_live());
}

/* And it is released exactly once, by the caller: both slots must be free
 * again after the holding scope ends. */
void test_returned_alias_is_released_by_the_caller(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_alias_return_is_not_leaked());
}

void test_returned_call_result_is_released_by_the_caller(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_opaque_call_return_is_not_leaked());
}
