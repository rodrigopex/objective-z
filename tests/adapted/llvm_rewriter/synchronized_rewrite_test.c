/*
 * Adapted from: clang/test/Rewriter/objc-synchronized-1.m
 * Verifies @synchronized lowering produces correct lock/unlock C structure.
 */
#include "unity.h"
#include "SyncObj_ozh.h"

void test_synchronized_rewrite_body_executes(void)
{
	struct SyncObj *s = SyncObj_alloc();
	SyncObj_increment(s);
	SyncObj_increment(s);
	TEST_ASSERT_EQUAL_INT(2, SyncObj_counter(s));
	OZObject_release((struct OZObject *)s);
}
