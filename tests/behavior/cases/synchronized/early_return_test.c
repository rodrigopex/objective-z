/* Behavior test: return inside @synchronized releases lock before returning */
#include "unity.h"
#include "EarlyRet_ozh.h"

void test_early_return_releases_lock(void)
{
	struct EarlyRet *e = EarlyRet_alloc();
	TEST_ASSERT_NOT_NULL(e);

	int result = EarlyRet_compute(e);
	TEST_ASSERT_EQUAL_INT(77, result);

	/* Verify ivar was set before return */
	TEST_ASSERT_EQUAL_INT(77, EarlyRet_value(e));

	OZObject_release((struct OZObject *)e);
}
