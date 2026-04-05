/*
 * Behavioral spec derived from: Apple objc4 property documentation
 * Verifies property attribute semantics: readwrite, readonly.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "PropAttrTest_ozh.h"

void test_readwrite_property(void)
{
	struct PropAttrTest *t = PropAttrTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	PropAttrTest_setReadwrite_(t, 42);
	TEST_ASSERT_EQUAL_INT(42, PropAttrTest_readwrite(t));
	OZObject_release((struct OZObject *)t);
}

void test_readonly_via_direct_ivar(void)
{
	struct PropAttrTest *t = PropAttrTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	PropAttrTest_setReadonlyDirect_(t, 99);
	TEST_ASSERT_EQUAL_INT(99, PropAttrTest_readonly(t));
	OZObject_release((struct OZObject *)t);
}
