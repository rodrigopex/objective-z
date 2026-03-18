/*
 * Regression test: block-typed ivar must generate valid C function pointer
 * declaration syntax — void (*_block)(...) not void (*)(...) _block.
 *
 * This test compiles and exercises OZDefer's block ivar through the
 * generated struct. A compilation failure here means the struct field
 * declaration regressed to invalid C syntax.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "BlockIvarTest_ozh.h"
#include "Foundation/OZDefer_ozh.h"

static int g_block_called = 0;

static void test_block_fn(struct OZObject *owner)
{
	(void)owner;
	g_block_called = 1;
}

void test_block_ivar_callable_through_struct(void)
{
	/* Directly assign and call the function pointer field.
	 * This only compiles if the struct declaration is correct. */
	struct OZDefer d;
	memset(&d, 0, sizeof(d));
	d._block = test_block_fn;
	d._owner = (struct OZObject *)0;

	g_block_called = 0;
	d._block(d._owner);
	TEST_ASSERT_EQUAL_INT(1, g_block_called);
}

void test_block_ivar_lifecycle(void)
{
	/* Full lifecycle: create OZDefer, store in owner, release owner. */
	struct OZDefer *defer = OZDefer_alloc();
	TEST_ASSERT_NOT_NULL(defer);
	defer->_block = test_block_fn;
	defer->_owner = (struct OZObject *)0;

	struct BlockIvarTest *t = BlockIvarTest_alloc();
	TEST_ASSERT_NOT_NULL(t);
	BlockIvarTest_initWithDefer_(t, defer);
	/* ivar assignment retains defer; release local ref so
	 * only the ivar holds ownership (refcount = 1). */
	OZObject_release((struct OZObject *)defer);

	g_block_called = 0;
	OZObject_release((struct OZObject *)t);
	TEST_ASSERT_EQUAL_INT(1, g_block_called);
}
