/* Behavior test: enum used in switch/case dispatches correctly */
#include "unity.h"
#include "EnumSwitchTest_ozh.h"

void test_enum_switch_red(void)
{
	struct EnumSwitchTest *t = EnumSwitchTest_alloc();
	EnumSwitchTest_classifyColor_(t, 0);
	TEST_ASSERT_EQUAL_INT(10, EnumSwitchTest_result(t));
	OZObject_release((struct OZObject *)t);
}

void test_enum_switch_green(void)
{
	struct EnumSwitchTest *t = EnumSwitchTest_alloc();
	EnumSwitchTest_classifyColor_(t, 1);
	TEST_ASSERT_EQUAL_INT(20, EnumSwitchTest_result(t));
	OZObject_release((struct OZObject *)t);
}

void test_enum_switch_blue(void)
{
	struct EnumSwitchTest *t = EnumSwitchTest_alloc();
	EnumSwitchTest_classifyColor_(t, 2);
	TEST_ASSERT_EQUAL_INT(30, EnumSwitchTest_result(t));
	OZObject_release((struct OZObject *)t);
}
