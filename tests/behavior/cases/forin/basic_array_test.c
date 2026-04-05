/* Behavior test: for-in iterates over OZArray elements */
#include "unity.h"
#include "oz_dispatch.h"
#include "IterTest_ozh.h"

void test_forin_sums_array_elements(void)
{
	struct IterTest *t = IterTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	IterTest_sumArray(t);
	TEST_ASSERT_EQUAL_INT(60, IterTest_sum(t));
	OZObject_release((struct OZObject *)t);
}
