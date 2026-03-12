/* Behavior test: dot-syntax maps to generated getter/setter */
#include "unity.h"
#include "Setting_ozh.h"

void test_dot_syntax_set_get(void)
{
	struct Setting *s = Setting_alloc();
	Setting_setBrightness_(s, 75);
	TEST_ASSERT_EQUAL_INT(75, Setting_brightness(s));
	OZObject_release((struct OZObject *)s);
}
