/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 * Verifies property setter retains new value correctly.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "Held_ozh.h"
#include "PropHolder_ozh.h"

void test_property_retains_value(void)
{
	struct PropHolder *h = PropHolder_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)h);

	struct Held *item = Held_alloc();
	Held_setTag_(item, 77);

	PropHolder_setItem_(h, item);
	TEST_ASSERT_EQUAL_INT(77, PropHolder_itemTag(h));

	OZObject_release((struct OZObject *)item);
	OZObject_release((struct OZObject *)h);
}
