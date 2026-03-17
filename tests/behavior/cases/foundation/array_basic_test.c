/* Behavior test: OZArray literal creation and access */
#include "unity.h"
#include "oz_dispatch.h"
#include "ArrayTest_ozh.h"

void test_array_literal_count(void)
{
	struct ArrayTest *t = ArrayTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_UINT(3, ArrayTest_literalCount(t));
	OZObject_release((struct OZObject *)t);
}

void test_array_first_element(void)
{
	struct ArrayTest *t = ArrayTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(42, ArrayTest_firstElement(t));
	OZObject_release((struct OZObject *)t);
}

void test_array_out_of_bounds_nil(void)
{
	struct ArrayTest *t = ArrayTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_TRUE(ArrayTest_outOfBoundsNil(t));
	OZObject_release((struct OZObject *)t);
}
