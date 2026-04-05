/* Behavior test: block references file-scope static variable */
#include "unity.h"
#include "StaticBlockTest_ozh.h"

void test_block_reads_static_var(void)
{
	struct StaticBlockTest *t = StaticBlockTest_alloc();
	TEST_ASSERT_NOT_NULL(t);

	StaticBlockTest_run(t);
	TEST_ASSERT_EQUAL_INT(15, StaticBlockTest_result(t));

	OZObject_release((struct OZObject *)t);
}
