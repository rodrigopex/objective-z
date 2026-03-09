/* Behavior test: assign property stores raw value (no retain) */
#include "unity.h"
#include "Holder.h"
#include "oz_mem_slabs.h"

void test_assign_stores_value(void)
{
	struct Holder *h = Holder_alloc();
	Holder_setValue_(h, 42);
	TEST_ASSERT_EQUAL_INT(42, Holder_value(h));
	OZObject_release((struct OZObject *)h);
}

void test_assign_overwrites_value(void)
{
	struct Holder *h = Holder_alloc();
	Holder_setValue_(h, 10);
	Holder_setValue_(h, 20);
	TEST_ASSERT_EQUAL_INT(20, Holder_value(h));
	OZObject_release((struct OZObject *)h);
}
