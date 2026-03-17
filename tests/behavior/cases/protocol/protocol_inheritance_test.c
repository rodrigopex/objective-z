/* Behavior test: protocol inheriting another protocol */
#include "unity.h"
#include "Athlete_ozh.h"

void test_protocol_inherited_method(void)
{
	struct Athlete *a = Athlete_alloc();
	TEST_ASSERT_EQUAL_INT(5, OZ_PROTOCOL_SEND_run((struct OZObject *)a));
	OZObject_release((struct OZObject *)a);
}

void test_protocol_own_method(void)
{
	struct Athlete *a = Athlete_alloc();
	TEST_ASSERT_EQUAL_INT(10, OZ_PROTOCOL_SEND_sprint((struct OZObject *)a));
	OZObject_release((struct OZObject *)a);
}
