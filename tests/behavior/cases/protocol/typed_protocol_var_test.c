/* Behavior test: protocol dispatch via typed variable */
#include "unity.h"
#include "Ruler_ozh.h"
#include "Scale_ozh.h"
#include "oz_mem_slabs.h"

void test_typed_protocol_ruler(void)
{
	struct OZObject *obj = (struct OZObject *)Ruler_alloc();
	TEST_ASSERT_EQUAL_INT(30, OZ_SEND_measure(obj));
	OZObject_release(obj);
}

void test_typed_protocol_scale(void)
{
	struct OZObject *obj = (struct OZObject *)Scale_alloc();
	TEST_ASSERT_EQUAL_INT(100, OZ_SEND_measure(obj));
	OZObject_release(obj);
}
