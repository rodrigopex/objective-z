/*
 * Adapted from: clang/test/Rewriter/blockcast3.mm
 * Verifies non-capturing block lowering produces correct function pointer.
 */
#include "unity.h"
#include "BlockObj_ozh.h"

void test_block_rewrite_executes(void)
{
	struct BlockObj *b = BlockObj_alloc();
	BlockObj_run(b);
	TEST_ASSERT_EQUAL_INT(42, BlockObj_result(b));
	OZObject_release((struct OZObject *)b);
}
