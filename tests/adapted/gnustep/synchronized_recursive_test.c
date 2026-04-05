/*
 * Adapted from: GNUstep libobjc2 — Test/Synchronized.m
 * Verifies recursive @synchronized on same object doesn't deadlock.
 */
#include "unity.h"
#include "RecursiveSyncTest_ozh.h"

void test_recursive_synchronized_no_deadlock(void)
{
	struct RecursiveSyncTest *t = RecursiveSyncTest_alloc();
	RecursiveSyncTest_recursiveLock(t);
	TEST_ASSERT_EQUAL_INT(2, RecursiveSyncTest_depth(t));
	OZObject_release((struct OZObject *)t);
}
