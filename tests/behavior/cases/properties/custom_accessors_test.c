/* Behavior test: custom getter/setter names and custom ivar via @synthesize */
#include "unity.h"
#include "Switch_ozh.h"

void test_custom_getter_name(void)
{
	struct Switch *sw = Switch_alloc();
	Switch_setEnabled_(sw, 1);
	TEST_ASSERT_EQUAL_INT(1, Switch_isEnabled(sw));
	OZObject_release((struct OZObject *)sw);
}

void test_custom_setter_name(void)
{
	struct Switch *sw = Switch_alloc();
	Switch_applySpeed_(sw, 50);
	TEST_ASSERT_EQUAL_INT(50, Switch_speed(sw));
	OZObject_release((struct OZObject *)sw);
}
