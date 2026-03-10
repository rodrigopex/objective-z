/* Behavior test: subclass overrides parent's accessor */
#include "unity.h"
#include "Square.h"
#include "oz_mem_slabs.h"

void test_override_returns_child_value(void)
{
	struct Square *sq = Square_alloc();
	TEST_ASSERT_EQUAL_INT(4, Square_sides(sq));
	OZObject_release((struct OZObject *)sq);
}

void test_parent_returns_default(void)
{
	struct Shape *sh = Shape_alloc();
	TEST_ASSERT_EQUAL_INT(0, Shape_sides(sh));
	OZObject_release((struct OZObject *)sh);
}
