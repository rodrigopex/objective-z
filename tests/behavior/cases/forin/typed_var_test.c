/* Behavior test: for-in with typed iteration variable (OZString *) */
#include "unity.h"
#include "oz_dispatch.h"
#include "TypedIterTest_ozh.h"

void test_forin_typed_var_iterates(void)
{
	struct TypedIterTest *t = TypedIterTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TypedIterTest_countStrings(t);
	TEST_ASSERT_EQUAL_INT(3, TypedIterTest_count(t));
	OZObject_release((struct OZObject *)t);
}
