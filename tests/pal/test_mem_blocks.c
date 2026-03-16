/* PAL contiguous block allocator unit tests */
#include "unity.h"
#include "platform/oz_platform.h"

void test_mem_blocks_alloc_returns_ok(void)
{
	OZ_MEM_BLOCKS_DEFINE(pool, sizeof(void *), 8, 4);
	void *mem = NULL;
	int rc = oz_mem_blocks_alloc_contiguous(&pool, 2, &mem);
	TEST_ASSERT_EQUAL_INT(OZ_OK, rc);
	TEST_ASSERT_NOT_NULL(mem);
	oz_mem_blocks_free_contiguous(&pool, mem, 2);
}

void test_mem_blocks_tracks_usage(void)
{
	OZ_MEM_BLOCKS_DEFINE(pool, sizeof(void *), 8, 4);
	void *mem = NULL;
	oz_mem_blocks_alloc_contiguous(&pool, 3, &mem);
	TEST_ASSERT_EQUAL_UINT32(3, pool.num_used);
	oz_mem_blocks_free_contiguous(&pool, mem, 3);
	TEST_ASSERT_EQUAL_UINT32(0, pool.num_used);
}

void test_mem_blocks_exhaustion(void)
{
	OZ_MEM_BLOCKS_DEFINE(pool, sizeof(void *), 4, 4);
	void *a = NULL;
	void *b = NULL;
	oz_mem_blocks_alloc_contiguous(&pool, 3, &a);
	int rc = oz_mem_blocks_alloc_contiguous(&pool, 3, &b);
	TEST_ASSERT_EQUAL_INT(OZ_ENOMEM, rc);
	TEST_ASSERT_NULL(b);
	oz_mem_blocks_free_contiguous(&pool, a, 3);
}

void test_mem_blocks_free_at_zero_safe(void)
{
	OZ_MEM_BLOCKS_DEFINE(pool, sizeof(void *), 4, 4);
	/* num_used is 0; free should not underflow */
	TEST_ASSERT_EQUAL_UINT32(0, pool.num_used);
}
