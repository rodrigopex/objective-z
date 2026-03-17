/*
 * Adapted from: clang/test/Rewriter/objc-modern-property-attributes.mm
 * License: Apache 2.0 with LLVM Exception
 * Verifies @property generates getter/setter via transpilation.
 */
#include "unity.h"
#include "Sensor_ozh.h"

void test_property_getter_setter(void)
{
	struct Sensor *s = Sensor_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)s);

	Sensor_setTemperature_(s, 25);
	TEST_ASSERT_EQUAL_INT(25, Sensor_temperature(s));

	OZObject_release((struct OZObject *)s);
}

void test_readonly_property_default_zero(void)
{
	struct Sensor *s = Sensor_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)s);

	TEST_ASSERT_EQUAL_INT(0, Sensor_humidity(s));

	OZObject_release((struct OZObject *)s);
}
