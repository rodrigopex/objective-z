/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 *
 * Proves: a strong object property holds a reference of its own -- the slab
 * slot is still outstanding after the only local reference has gone out of
 * scope.
 * Does not prove: the refcount value, or release-old-on-overwrite. Both are
 * `tests/behavior/cases/properties/strong_vs_assign_test.c`, which reads
 * `oz_retain_count`. This case is the complementary allocator-side oracle.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "Held_ozh.h"
#include "PropHolder_ozh.h"

void test_strong_property_holds_its_target(void)
{
	struct PropHolder *h = PropHolder_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)h);

	PropHolder_run(h);

	/* The control first: assert the pool really is one slot, or the claim
	 * below proves nothing (#455). */
	TEST_ASSERT_EQUAL_INT(1, PropHolder_poolIsOne(h));

	/* The property stored the object, not just a retain. */
	TEST_ASSERT_EQUAL_INT(77, PropHolder_tagThroughProperty(h));

	/* The claim: the slot is still held once the local reference is gone. */
	TEST_ASSERT_EQUAL_INT(1, PropHolder_propertyKeepsItAlive(h));

	OZObject_release((struct OZObject *)h);
}
