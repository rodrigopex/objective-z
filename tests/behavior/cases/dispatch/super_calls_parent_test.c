/* Behavior test: [super init] calls parent's init */
#include "unity.h"
#include "Child_ozh.h"

void test_super_calls_parent(void)
{
	struct Child *c = Child_alloc();
	OZ_SEND_init((struct OZObject *)c);

	/* Parent init sets baseVal=10, child init sets childVal=20 */
	TEST_ASSERT_EQUAL_INT(10, Base_baseVal((struct Base *)c));
	TEST_ASSERT_EQUAL_INT(20, Child_childVal(c));

	OZObject_release((struct OZObject *)c);
}
