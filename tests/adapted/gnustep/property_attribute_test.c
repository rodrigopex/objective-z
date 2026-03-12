/*
 * Adapted from: GNUstep libobjc2 — Test/PropertyAttributeTest.m
 * License: MIT
 * Verifies property getter/setter attribute behavior.
 */
#include "unity.h"
#include "Config_ozh.h"

void test_property_default_zero(void)
{
	struct Config *c = Config_alloc();
	OZ_SEND_init((struct OZObject *)c);

	TEST_ASSERT_EQUAL_INT(0, Config_level(c));
	TEST_ASSERT_EQUAL_INT(0, Config_mode(c));

	OZObject_release((struct OZObject *)c);
}

void test_property_set_and_get(void)
{
	struct Config *c = Config_alloc();
	OZ_SEND_init((struct OZObject *)c);

	Config_setLevel_(c, 5);
	Config_setMode_(c, 3);
	TEST_ASSERT_EQUAL_INT(5, Config_level(c));
	TEST_ASSERT_EQUAL_INT(3, Config_mode(c));

	OZObject_release((struct OZObject *)c);
}
