/* Behavior test: enum used as ivar type — set and read */
#include "unity.h"
#include "EnumIvarTest_ozh.h"

void test_enum_ivar_set_and_get(void)
{
	struct EnumIvarTest *t = EnumIvarTest_alloc();

	EnumIvarTest_setDirection_(t, 2);
	TEST_ASSERT_EQUAL_INT(2, EnumIvarTest_direction(t));

	EnumIvarTest_setDirection_(t, 0);
	TEST_ASSERT_EQUAL_INT(0, EnumIvarTest_direction(t));

	OZObject_release((struct OZObject *)t);
}
