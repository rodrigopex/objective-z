/* Behavior test: block passed as method parameter runs inside method body */
#include "unity.h"
#include "BlockParamTest_ozh.h"

static int double_it(int x)
{
	return x * 2;
}

void test_block_as_method_param(void)
{
	struct BlockParamTest *t = BlockParamTest_alloc();
	TEST_ASSERT_NOT_NULL(t);

	BlockParamTest_applyBlock_toValue_(t, double_it, 21);
	TEST_ASSERT_EQUAL_INT(42, BlockParamTest_computed(t));

	OZObject_release((struct OZObject *)t);
}
