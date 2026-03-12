/* Behavior test: getter/setter methods work correctly */
#include "unity.h"
#include "Config_ozh.h"

void test_getter_returns_default(void)
{
	struct Config *c = Config_alloc();
	OZ_SEND_init((struct OZObject *)c);
	TEST_ASSERT_EQUAL_INT(0, Config_level(c));
	OZObject_release((struct OZObject *)c);
}

void test_setter_then_getter(void)
{
	struct Config *c = Config_alloc();
	OZ_SEND_init((struct OZObject *)c);
	Config_setLevel_(c, 99);
	TEST_ASSERT_EQUAL_INT(99, Config_level(c));
	OZObject_release((struct OZObject *)c);
}
