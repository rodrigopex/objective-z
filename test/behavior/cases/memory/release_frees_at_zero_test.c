/* Behavior test: release at rc=1 frees object (slab block returned) */
#include "unity.h"
#include "Token.h"
#include "oz_mem_slabs.h"

void test_release_frees_at_zero(void)
{
	/* 1-block slab: alloc, release, re-alloc proves block was freed */
	struct Token *t1 = Token_alloc();
	TEST_ASSERT_NOT_NULL(t1);

	OZObject_release((struct OZObject *)t1);

	struct Token *t2 = Token_alloc();
	TEST_ASSERT_NOT_NULL(t2);
	OZObject_release((struct OZObject *)t2);
}
