/* Behavior test: OZString length and cString inline fast paths */
#include "unity.h"
#include "oz_dispatch.h"
#include "StringAccessTest_ozh.h"

void test_string_inline_length(void)
{
	struct StringAccessTest *t = StringAccessTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	StringAccessTest_run(t);
	TEST_ASSERT_EQUAL_UINT(5, StringAccessTest_len(t));
	OZObject_release((struct OZObject *)t);
}

void test_string_inline_cstring_valid(void)
{
	struct StringAccessTest *t = StringAccessTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	StringAccessTest_run(t);
	TEST_ASSERT_TRUE(StringAccessTest_cStringValid(t));
	OZObject_release((struct OZObject *)t);
}
