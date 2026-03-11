/* Behavior test: retain twice, release twice — freed only on last */
#include "unity.h"
#include "Handle_ozh.h"
#include "oz_mem_slabs.h"

void test_nested_retain_release(void)
{
	struct Handle *h = Handle_alloc();
	TEST_ASSERT_EQUAL_UINT(1, __objc_refcount_get(h));

	OZObject_retain((struct OZObject *)h);
	TEST_ASSERT_EQUAL_UINT(2, __objc_refcount_get(h));

	OZObject_retain((struct OZObject *)h);
	TEST_ASSERT_EQUAL_UINT(3, __objc_refcount_get(h));

	OZObject_release((struct OZObject *)h);
	TEST_ASSERT_EQUAL_UINT(2, __objc_refcount_get(h));

	OZObject_release((struct OZObject *)h);
	TEST_ASSERT_EQUAL_UINT(1, __objc_refcount_get(h));

	/* Final release triggers dealloc */
	OZObject_release((struct OZObject *)h);
}
