/* Behavior test: one class conforms to two protocols */
#include "unity.h"
#include "Stream_ozh.h"
#include "oz_mem_slabs.h"

void test_stream_read_via_protocol(void)
{
	struct Stream *s = Stream_alloc();
	TEST_ASSERT_EQUAL_INT(1, OZ_SEND_read((struct OZObject *)s));
	OZObject_release((struct OZObject *)s);
}

void test_stream_write_via_protocol(void)
{
	struct Stream *s = Stream_alloc();
	TEST_ASSERT_EQUAL_INT(2, OZ_SEND_write((struct OZObject *)s));
	OZObject_release((struct OZObject *)s);
}
