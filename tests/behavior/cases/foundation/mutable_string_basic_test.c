/* Behavior test: OZMutableString basic operations */
#include "unity.h"
#include "oz_dispatch.h"
#include "MutableStringTest_ozh.h"

void test_init_from_cstring(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_initFromCString(t);
	TEST_ASSERT_EQUAL_STRING("hello", s);
	OZObject_release((struct OZObject *)t);
}

void test_init_from_cstring_length(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	unsigned int len = MutableStringTest_initFromCStringLength(t);
	TEST_ASSERT_EQUAL_UINT(5, len);
	OZObject_release((struct OZObject *)t);
}

void test_init_from_ozstring(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_initFromOZString(t);
	TEST_ASSERT_EQUAL_STRING("world", s);
	OZObject_release((struct OZObject *)t);
}

void test_init_with_capacity(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_initWithCapacity(t);
	TEST_ASSERT_EQUAL_STRING("reserved", s);
	OZObject_release((struct OZObject *)t);
}

void test_append_cstring(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_appendCString(t);
	TEST_ASSERT_EQUAL_STRING("hello, world", s);
	OZObject_release((struct OZObject *)t);
}

void test_append_string(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_appendString(t);
	TEST_ASSERT_EQUAL_STRING("hello, world", s);
	OZObject_release((struct OZObject *)t);
}

void test_append_grow(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_appendGrow(t);
	TEST_ASSERT_EQUAL_STRING("abcdefghijklmnopqrstuvwxyz", s);
	OZObject_release((struct OZObject *)t);
}

void test_set_string_replace(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_setStringReplace(t);
	TEST_ASSERT_EQUAL_STRING("new", s);
	OZObject_release((struct OZObject *)t);
}

void test_set_string_nil(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	const char *s = MutableStringTest_setStringNil(t);
	TEST_ASSERT_EQUAL_STRING("", s);
	OZObject_release((struct OZObject *)t);
}

void test_has_prefix(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_TRUE(MutableStringTest_hasPrefixTrue(t));
	OZObject_release((struct OZObject *)t);
}

void test_has_suffix(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_TRUE(MutableStringTest_hasSuffixTrue(t));
	OZObject_release((struct OZObject *)t);
}

void test_is_equal_to_string(void)
{
	struct MutableStringTest *t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	TEST_ASSERT_TRUE(MutableStringTest_isEqualToStringTrue(t));
	OZObject_release((struct OZObject *)t);
}
