/* Behavior test: OZDictionary literal creation and access */
#include "unity.h"
#include "oz_dispatch.h"
#include "DictTest_ozh.h"

void test_dictionary_literal_count(void)
{
	struct DictTest *t = DictTest_alloc();
	OZ_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_UINT(2, DictTest_literalCount(t));
	OZObject_release((struct OZObject *)t);
}

void test_dictionary_value_for_key(void)
{
	struct DictTest *t = DictTest_alloc();
	OZ_SEND_init((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(99, DictTest_valueForKey(t));
	OZObject_release((struct OZObject *)t);
}

void test_dictionary_missing_key_nil(void)
{
	struct DictTest *t = DictTest_alloc();
	OZ_SEND_init((struct OZObject *)t);
	TEST_ASSERT_TRUE(DictTest_missingKeyNil(t));
	OZObject_release((struct OZObject *)t);
}
