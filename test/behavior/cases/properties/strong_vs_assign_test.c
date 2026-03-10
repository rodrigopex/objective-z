/* Behavior test: strong property retains, assign does not */
#include "unity.h"
#include "Item.h"
#include "Holder.h"
#include "oz_mem_slabs.h"

void test_assign_stores_value(void)
{
	struct Holder *h = Holder_alloc();
	Holder_setValue_(h, 42);
	TEST_ASSERT_EQUAL_INT(42, Holder_value(h));
	OZObject_release((struct OZObject *)h);
}

void test_strong_retains_on_set(void)
{
	struct Item *it = Item_alloc();
	OZ_SEND_init((struct OZObject *)it);
	/* rc=1 after alloc */
	TEST_ASSERT_EQUAL_INT(1, OZObject_retainCount((struct OZObject *)it));

	struct Holder *h = Holder_alloc();
	Holder_setItem_(h, it);
	/* strong setter retains: rc=2 */
	TEST_ASSERT_EQUAL_INT(2, OZObject_retainCount((struct OZObject *)it));

	OZObject_release((struct OZObject *)h);
	/* dealloc releases ivar: rc=1 */
	TEST_ASSERT_EQUAL_INT(1, OZObject_retainCount((struct OZObject *)it));

	OZObject_release((struct OZObject *)it);
}

void test_strong_releases_old_on_overwrite(void)
{
	struct Item *a = Item_alloc();
	OZ_SEND_init((struct OZObject *)a);
	struct Item *b = Item_alloc();
	OZ_SEND_init((struct OZObject *)b);

	struct Holder *h = Holder_alloc();
	Holder_setItem_(h, a);
	TEST_ASSERT_EQUAL_INT(2, OZObject_retainCount((struct OZObject *)a));

	/* overwrite: releases a, retains b */
	Holder_setItem_(h, b);
	TEST_ASSERT_EQUAL_INT(1, OZObject_retainCount((struct OZObject *)a));
	TEST_ASSERT_EQUAL_INT(2, OZObject_retainCount((struct OZObject *)b));

	OZObject_release((struct OZObject *)h);
	OZObject_release((struct OZObject *)a);
	OZObject_release((struct OZObject *)b);
}
