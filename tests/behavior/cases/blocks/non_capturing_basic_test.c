/* Behavior test: non-capturing block compiles and executes correctly */
#include "unity.h"
#include "BlockBasicTest_ozh.h"

void test_non_capturing_block_executes(void)
{
	struct BlockBasicTest *t = BlockBasicTest_alloc();
	TEST_ASSERT_NOT_NULL(t);

	BlockBasicTest_run(t);
	TEST_ASSERT_EQUAL_INT(49, BlockBasicTest_result(t));

	OZObject_release((struct OZObject *)t);
}
