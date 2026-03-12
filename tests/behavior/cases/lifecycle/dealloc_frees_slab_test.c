/* Behavior test: release returns slab block for reuse */
#include "unity.h"
#include "Slot_ozh.h"

void test_dealloc_returns_slab_block(void)
{
	/* Slab has 1 block — alloc, release, re-alloc must succeed */
	struct Slot *s1 = Slot_alloc();
	TEST_ASSERT_NOT_NULL(s1);
	OZObject_release((struct OZObject *)s1);

	struct Slot *s2 = Slot_alloc();
	TEST_ASSERT_NOT_NULL(s2);
	OZObject_release((struct OZObject *)s2);
}
