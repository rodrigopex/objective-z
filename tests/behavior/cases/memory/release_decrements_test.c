/* Behavior test: release decrements refcount without dealloc when rc > 1 */
#include "unity.h"
#include "Counter_ozh.h"
#include "oz_mem_slabs.h"

void test_release_decrements_refcount(void)
{
	struct Counter *c = Counter_alloc();
	OZObject_retain((struct OZObject *)c);
	OZObject_retain((struct OZObject *)c);
	TEST_ASSERT_EQUAL_UINT(3, __objc_refcount_get(c));

	OZObject_release((struct OZObject *)c);
	TEST_ASSERT_EQUAL_UINT(2, __objc_refcount_get(c));

	OZObject_release((struct OZObject *)c);
	TEST_ASSERT_EQUAL_UINT(1, __objc_refcount_get(c));

	OZObject_release((struct OZObject *)c);
}
