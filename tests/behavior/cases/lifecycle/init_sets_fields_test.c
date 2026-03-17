/* Behavior test: init sets ivar defaults */
#include "unity.h"
#include "Gadget_ozh.h"

void test_init_sets_value(void)
{
	struct Gadget *g = Gadget_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)g);
	TEST_ASSERT_EQUAL_INT(42, Gadget_value(g));
	OZObject_release((struct OZObject *)g);
}

void test_init_sets_ready(void)
{
	struct Gadget *g = Gadget_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)g);
	TEST_ASSERT_EQUAL_INT(1, Gadget_ready(g));
	OZObject_release((struct OZObject *)g);
}
