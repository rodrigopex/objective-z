/* Behavior test: readonly — getter exists, no setter */
#include "unity.h"
#include "Stamp_ozh.h"

void test_readonly_getter(void)
{
	struct Stamp *s = Stamp_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)s);
	TEST_ASSERT_EQUAL_INT(999, Stamp_serial(s));
	OZObject_release((struct OZObject *)s);
}
