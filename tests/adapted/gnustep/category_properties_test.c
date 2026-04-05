/*
 * Adapted from: GNUstep libobjc2 — Test/category_properties.m
 * Verifies category-defined setter/getter are functional.
 */
#include "unity.h"
#include "Gadget_ozh.h"

void test_category_property_set_get(void)
{
	struct Gadget *g = Gadget_alloc();
	Gadget_setPower_(g, 50);
	TEST_ASSERT_EQUAL_INT(50, Gadget_power(g));
	TEST_ASSERT_EQUAL_INT(100, Gadget_doublePower(g));
	OZObject_release((struct OZObject *)g);
}
