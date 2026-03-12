/* Behavior test: alloc returns NULL when slab is exhausted */
#include "unity.h"
#include "Box_ozh.h"

void test_alloc_failure_returns_null(void)
{
	/* Slab has only 1 block */
	struct Box *b1 = Box_alloc();
	TEST_ASSERT_NOT_NULL(b1);

	/* Second alloc must fail */
	struct Box *b2 = Box_alloc();
	TEST_ASSERT_NULL(b2);

	OZObject_release((struct OZObject *)b1);
}
