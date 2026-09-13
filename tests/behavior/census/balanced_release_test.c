/* Census control: the same shape as `leaks_one_object`, released (#451).
 *
 * The control for the leak fixture next to it. Without this, "the census
 * reported a leak" would be consistent with a census that reports a leak
 * unconditionally -- which is the failure this repo keeps hitting, a check
 * that passes while the property it names is gone. */
#include "unity.h"
#include "Tidy_ozh.h"

void test_the_allocation_itself_succeeds(void)
{
	struct Tidy *t = Tidy_alloc();

	TEST_ASSERT_NOT_NULL(t);
	OZObject_release((struct OZObject *)t);
}
