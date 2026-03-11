/* Behavior test: method call routes to correct implementation */
#include "unity.h"
#include "Speaker_ozh.h"
#include "oz_mem_slabs.h"

void test_send_routes_correct(void)
{
	struct Speaker *s = Speaker_alloc();
	OZ_SEND_init((struct OZObject *)s);

	TEST_ASSERT_EQUAL_INT(0, Speaker_spoken(s));
	Speaker_speak(s);
	TEST_ASSERT_EQUAL_INT(1, Speaker_spoken(s));

	OZObject_release((struct OZObject *)s);
}
