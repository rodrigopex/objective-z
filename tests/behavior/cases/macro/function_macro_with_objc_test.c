/* Behavior test: function-like macro expands correctly with ObjC method arg */
#include "unity.h"
#include "MacroFuncTest_ozh.h"

void test_function_macro_doubles(void)
{
	struct MacroFuncTest *t = MacroFuncTest_alloc();
	MacroFuncTest_runWithValue_(t, 21);
	TEST_ASSERT_EQUAL_INT(42, MacroFuncTest_doubled(t));
	OZObject_release((struct OZObject *)t);
}
