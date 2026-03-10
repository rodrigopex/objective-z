/*
 * Adapted from: GNUstep libobjc2 — Test/BasicTest.m
 * License: MIT
 * Verifies simple message send and return value.
 */
#include "unity.h"
#include "Counter.h"
#include "oz_mem_slabs.h"

void test_message_send_and_return(void)
{
	struct Counter *c = Counter_alloc();
	OZ_SEND_init((struct OZObject *)c);

	TEST_ASSERT_EQUAL_INT(0, Counter_count(c));
	Counter_increment(c);
	Counter_increment(c);
	TEST_ASSERT_EQUAL_INT(2, Counter_count(c));
	Counter_decrement(c);
	TEST_ASSERT_EQUAL_INT(1, Counter_count(c));

	OZObject_release((struct OZObject *)c);
}
