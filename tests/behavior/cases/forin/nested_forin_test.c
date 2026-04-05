/* Behavior test: nested for-in loops don't interfere */
#include "unity.h"
#include "oz_dispatch.h"
#include "NestedIterTest_ozh.h"

void test_nested_forin_computes_correctly(void)
{
	struct NestedIterTest *t = NestedIterTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	NestedIterTest_nestedIteration(t);
	/* outer [1,2] x inner [10,20]:
	 * (1+10) + (1+20) + (2+10) + (2+20) = 11 + 21 + 12 + 22 = 66 */
	TEST_ASSERT_EQUAL_INT(66, NestedIterTest_total(t));
	OZObject_release((struct OZObject *)t);
}
