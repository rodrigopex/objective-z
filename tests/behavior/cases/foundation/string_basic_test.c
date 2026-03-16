/* Behavior test: OZString basic operations */
#include "unity.h"
#include "oz_dispatch.h"
#include "StringTest_ozh.h"

void test_string_cstr(void)
{
	struct StringTest *t = StringTest_alloc();
	OZ_SEND_init((struct OZObject *)t);
	const char *s = StringTest_getHello(t);
	TEST_ASSERT_EQUAL_STRING("hello", s);
	OZObject_release((struct OZObject *)t);
}

void test_string_length(void)
{
	struct StringTest *t = StringTest_alloc();
	OZ_SEND_init((struct OZObject *)t);
	unsigned int len = StringTest_helloLength(t);
	TEST_ASSERT_EQUAL_UINT(5, len);
	OZObject_release((struct OZObject *)t);
}

void test_string_equal_same(void)
{
	struct StringTest *t = StringTest_alloc();
	OZ_SEND_init((struct OZObject *)t);
	TEST_ASSERT_TRUE(StringTest_sameStringEqual(t));
	OZObject_release((struct OZObject *)t);
}
