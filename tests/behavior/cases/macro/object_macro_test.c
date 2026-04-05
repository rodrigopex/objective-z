/* Behavior test: object-like macro value correct at runtime */
#include "unity.h"
#include "MacroObjTest_ozh.h"

void test_object_macro_value(void)
{
	struct MacroObjTest *t = MacroObjTest_alloc();
	MacroObjTest_run(t);
	TEST_ASSERT_EQUAL_INT(10, MacroObjTest_value(t));
	OZObject_release((struct OZObject *)t);
}
