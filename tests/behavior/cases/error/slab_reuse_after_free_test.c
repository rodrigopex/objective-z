/* Behavior test: slab blocks are reusable after release */
#include "unity.h"
#include "Gadget_ozh.h"
#include "oz_mem_slabs.h"

void test_slab_exhaustion_returns_null(void)
{
	struct Gadget *g1 = Gadget_alloc();
	struct Gadget *g2 = Gadget_alloc();
	TEST_ASSERT_NOT_NULL(g1);
	TEST_ASSERT_NOT_NULL(g2);

	/* Pool of 2 is full */
	struct Gadget *g3 = Gadget_alloc();
	TEST_ASSERT_NULL(g3);

	OZObject_release((struct OZObject *)g1);
	OZObject_release((struct OZObject *)g2);
}

void test_slab_reuse_after_release(void)
{
	struct Gadget *g1 = Gadget_alloc();
	TEST_ASSERT_NOT_NULL(g1);
	Gadget_setTag_(g1, 99);
	TEST_ASSERT_EQUAL_INT(99, Gadget_tag(g1));

	OZObject_release((struct OZObject *)g1);

	/* Block should be available again */
	struct Gadget *g2 = Gadget_alloc();
	TEST_ASSERT_NOT_NULL(g2);

	OZObject_release((struct OZObject *)g2);
}
