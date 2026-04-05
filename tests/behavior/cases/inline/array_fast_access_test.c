/* Behavior test: OZArray count and objectAtIndex inline fast paths */
#include "unity.h"
#include "oz_dispatch.h"
#include "ArrayAccessTest_ozh.h"

void test_array_inline_count(void)
{
	struct ArrayAccessTest *t = ArrayAccessTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	ArrayAccessTest_run(t);
	TEST_ASSERT_EQUAL_UINT(3, ArrayAccessTest_count(t));
	OZObject_release((struct OZObject *)t);
}

void test_array_inline_object_at_index(void)
{
	struct ArrayAccessTest *t = ArrayAccessTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	ArrayAccessTest_run(t);
	TEST_ASSERT_EQUAL_INT(100, ArrayAccessTest_firstVal(t));
	OZObject_release((struct OZObject *)t);
}
