/* Behavior test: OZNumber boxing */
#include "unity.h"
#include "oz_dispatch.h"
#include "NumTest_ozh.h"

void test_boxed(void)
{
	struct NumTest *t = NumTest_alloc();
	OZ_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(42, NumTest_boxed(t));
	OZObject_release((struct OZObject *)t);
}
