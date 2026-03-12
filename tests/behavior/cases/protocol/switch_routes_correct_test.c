/* Behavior test: protocol dispatch routes to correct class */
#include "unity.h"
#include "LightSwitch_ozh.h"
#include "Fan_ozh.h"

void test_protocol_routes_light(void)
{
	struct LightSwitch *ls = LightSwitch_alloc();
	int result = OZ_SEND_toggle((struct OZObject *)ls);
	TEST_ASSERT_EQUAL_INT(1, result);
	OZObject_release((struct OZObject *)ls);
}

void test_protocol_routes_fan(void)
{
	struct Fan *f = Fan_alloc();
	int result = OZ_SEND_toggle((struct OZObject *)f);
	TEST_ASSERT_EQUAL_INT(11, result);
	OZObject_release((struct OZObject *)f);
}
