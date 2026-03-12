/* Behavior test: 4-level inheritance chain */
#include "unity.h"
#include "Level4_ozh.h"

void test_level4_depth(void)
{
	struct Level4 *l4 = Level4_alloc();
	TEST_ASSERT_EQUAL_INT(4, Level4_depth(l4));
	OZObject_release((struct OZObject *)l4);
}

void test_level3_depth(void)
{
	struct Level3 *l3 = Level3_alloc();
	TEST_ASSERT_EQUAL_INT(3, Level3_depth(l3));
	OZObject_release((struct OZObject *)l3);
}

void test_level1_depth(void)
{
	struct Level1 *l1 = Level1_alloc();
	TEST_ASSERT_EQUAL_INT(1, Level1_depth(l1));
	OZObject_release((struct OZObject *)l1);
}
