/* Behavior test: atomic properties (default, no nonatomic) */
#include "unity.h"
#include "Counter.h"
#include "Container.h"
#include "oz_mem_slabs.h"

void test_atomic_assign_set_get(void)
{
	struct Counter *c = Counter_alloc();
	Counter_setCount_(c, 77);
	TEST_ASSERT_EQUAL_INT(77, Counter_count(c));
	OZObject_release((struct OZObject *)c);
}

void test_atomic_strong_retains(void)
{
	struct Counter *c = Counter_alloc();
	OZ_SEND_init((struct OZObject *)c);
	TEST_ASSERT_EQUAL_INT(1, OZObject_retainCount((struct OZObject *)c));

	struct Container *ct = Container_alloc();
	Container_setCounter_(ct, c);
	/* atomic strong setter retains */
	TEST_ASSERT_EQUAL_INT(2, OZObject_retainCount((struct OZObject *)c));

	OZObject_release((struct OZObject *)ct);
	/* dealloc releases ivar */
	TEST_ASSERT_EQUAL_INT(1, OZObject_retainCount((struct OZObject *)c));

	OZObject_release((struct OZObject *)c);
}

void test_atomic_strong_releases_old(void)
{
	struct Counter *a = Counter_alloc();
	OZ_SEND_init((struct OZObject *)a);
	struct Counter *b = Counter_alloc();
	OZ_SEND_init((struct OZObject *)b);

	struct Container *ct = Container_alloc();
	Container_setCounter_(ct, a);
	Container_setCounter_(ct, b);
	TEST_ASSERT_EQUAL_INT(1, OZObject_retainCount((struct OZObject *)a));
	TEST_ASSERT_EQUAL_INT(2, OZObject_retainCount((struct OZObject *)b));

	OZObject_release((struct OZObject *)ct);
	OZObject_release((struct OZObject *)a);
	OZObject_release((struct OZObject *)b);
}
