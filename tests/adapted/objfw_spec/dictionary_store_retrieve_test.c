/*
 * Behavioral spec derived from: ObjFW OFDictionaryTests.m
 * Verifies OZDictionary store/retrieve/missing key behavior.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "DictTest_ozh.h"

void test_dict_store_retrieve(void)
{
	struct DictTest *t = DictTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	DictTest_run(t);
	TEST_ASSERT_EQUAL_INT(42, DictTest_storedVal(t));
	OZObject_release((struct OZObject *)t);
}

void test_dict_missing_key_nil(void)
{
	struct DictTest *t = DictTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	DictTest_run(t);
	TEST_ASSERT_TRUE(DictTest_missingNil(t));
	OZObject_release((struct OZObject *)t);
}

void test_dict_count(void)
{
	struct DictTest *t = DictTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	DictTest_run(t);
	TEST_ASSERT_EQUAL_UINT(1, DictTest_count(t));
	OZObject_release((struct OZObject *)t);
}
