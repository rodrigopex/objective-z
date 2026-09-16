/* Behavior test: OZMutableString basic operations */
#include "unity.h"
#include "oz_dispatch.h"
#include "MutableStringTest_ozh.h"

static struct MutableStringTest *t;

void setUp(void)
{
	t = MutableStringTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
}

void tearDown(void)
{
	OZObject_release((struct OZObject *)t);
}

void test_init_from_cstring(void)
{
	MutableStringTest_buildFromCString(t);
	TEST_ASSERT_EQUAL_STRING("hello", MutableStringTest_result(t));
	TEST_ASSERT_EQUAL_UINT(5, MutableStringTest_resultLength(t));
}

void test_init_from_ozstring(void)
{
	MutableStringTest_buildFromOZString(t);
	TEST_ASSERT_EQUAL_STRING("world", MutableStringTest_result(t));
}

void test_init_with_capacity(void)
{
	MutableStringTest_buildWithCapacity(t);
	TEST_ASSERT_EQUAL_STRING("reserved", MutableStringTest_result(t));
}

void test_append_cstring(void)
{
	MutableStringTest_buildAndAppendCString(t);
	TEST_ASSERT_EQUAL_STRING("hello, world", MutableStringTest_result(t));
}

void test_append_string(void)
{
	MutableStringTest_buildAndAppendString(t);
	TEST_ASSERT_EQUAL_STRING("hello, world", MutableStringTest_result(t));
}

void test_append_grow(void)
{
	MutableStringTest_buildAndAppendGrow(t);
	TEST_ASSERT_EQUAL_STRING("abcdefghijklmnopqrstuvwxyz", MutableStringTest_result(t));
}

void test_set_string_replace(void)
{
	MutableStringTest_buildAndSetString(t);
	TEST_ASSERT_EQUAL_STRING("new", MutableStringTest_result(t));
}

void test_set_string_nil(void)
{
	MutableStringTest_buildAndSetStringNil(t);
	TEST_ASSERT_EQUAL_STRING("", MutableStringTest_result(t));
}

void test_has_prefix(void)
{
	TEST_ASSERT_TRUE(MutableStringTest_hasPrefixTrue(t));
}

void test_has_suffix(void)
{
	TEST_ASSERT_TRUE(MutableStringTest_hasSuffixTrue(t));
}

void test_is_equal_to_string(void)
{
	TEST_ASSERT_TRUE(MutableStringTest_isEqualToStringTrue(t));
}

void test_reinitialise_replaces_content_without_leaking(void)
{
	MutableStringTest_buildAndReinitialise(t);
	TEST_ASSERT_EQUAL_STRING("second", MutableStringTest_result(t));
	TEST_ASSERT_EQUAL_UINT(6, MutableStringTest_resultLength(t));
}

/*
 * #542: a receiver that never ran an initialiser has `_capacity == 0` and
 * `_data == NULL` from `+alloc`'s memset. Both grow loops doubled from
 * `_capacity`, so they never terminated, and `-setString:nil` wrote
 * through the NULL. Reaching these assertions at all is most of the
 * claim; the contents pin that the floored grow allocates a usable
 * buffer rather than merely terminating.
 */
void test_append_cstring_to_uninitialised_receiver(void)
{
	MutableStringTest_appendCStringToUninitialised(t);
	TEST_ASSERT_EQUAL_STRING("grown from nothing", MutableStringTest_result(t));
	TEST_ASSERT_EQUAL_UINT(18, MutableStringTest_resultLength(t));
}

void test_set_string_on_uninitialised_receiver(void)
{
	MutableStringTest_setStringOnUninitialised(t);
	TEST_ASSERT_EQUAL_STRING("assigned from nothing", MutableStringTest_result(t));
	TEST_ASSERT_EQUAL_UINT(21, MutableStringTest_resultLength(t));
}

void test_set_string_nil_on_uninitialised_receiver(void)
{
	MutableStringTest_setStringNilOnUninitialised(t);
	TEST_ASSERT_EQUAL_UINT(0, MutableStringTest_resultLength(t));
}
