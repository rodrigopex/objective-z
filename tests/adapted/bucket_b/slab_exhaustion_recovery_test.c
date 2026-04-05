/*
 * Adapted from: tests/objc-reference/runtime/memory/src/main.c
 * Verifies slab recovers after exhaustion when a block is freed.
 */
#include "unity.h"
#include "ExhaustTest_ozh.h"

void test_slab_exhaustion_recovery(void)
{
	struct ExhaustTest *t = ExhaustTest_alloc();
	ExhaustTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, ExhaustTest_recoveryOk(t));
	OZObject_release((struct OZObject *)t);
}
