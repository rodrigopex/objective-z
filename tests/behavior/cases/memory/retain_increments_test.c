/* Behavior test: retain increments refcount */
#include "unity.h"
#include "Node_ozh.h"
#include "oz_mem_slabs.h"

void test_retain_increments_refcount(void)
{
	struct Node *n = Node_alloc();
	TEST_ASSERT_EQUAL_UINT(1, __objc_refcount_get(n));

	OZObject_retain((struct OZObject *)n);
	TEST_ASSERT_EQUAL_UINT(2, __objc_refcount_get(n));

	OZObject_retain((struct OZObject *)n);
	TEST_ASSERT_EQUAL_UINT(3, __objc_refcount_get(n));

	/* Balance releases */
	OZObject_release((struct OZObject *)n);
	OZObject_release((struct OZObject *)n);
	OZObject_release((struct OZObject *)n);
}
