/* PAL slab allocator unit tests */
#include "unity.h"
#include "platform/oz_platform.h"

OZ_SLAB_DEFINE(test_slab, 64, 4, 4);

void test_slab_alloc_returns_ok(void)
{
	void *mem = NULL;
	int rc = oz_slab_alloc(&test_slab, &mem);
	TEST_ASSERT_EQUAL_INT(OZ_OK, rc);
	TEST_ASSERT_NOT_NULL(mem);
	oz_slab_free(&test_slab, mem);
}

void test_slab_alloc_tracks_usage(void)
{
	void *a = NULL;
	void *b = NULL;
	oz_slab_alloc(&test_slab, &a);
	oz_slab_alloc(&test_slab, &b);
	TEST_ASSERT_EQUAL_UINT32(2, test_slab.num_used);
	oz_slab_free(&test_slab, b);
	TEST_ASSERT_EQUAL_UINT32(1, test_slab.num_used);
	oz_slab_free(&test_slab, a);
	TEST_ASSERT_EQUAL_UINT32(0, test_slab.num_used);
}

void test_slab_free_decrements(void)
{
	void *mem = NULL;
	oz_slab_alloc(&test_slab, &mem);
	uint32_t before = test_slab.num_used;
	oz_slab_free(&test_slab, mem);
	TEST_ASSERT_EQUAL_UINT32(before - 1, test_slab.num_used);
}

void test_slab_exhaustion_returns_enomem(void)
{
	/* Reset slab to known state */
	OZ_SLAB_DEFINE(small_slab, 16, 2, 4);
	void *a = NULL;
	void *b = NULL;
	void *c = NULL;

	TEST_ASSERT_EQUAL_INT(OZ_OK, oz_slab_alloc(&small_slab, &a));
	TEST_ASSERT_EQUAL_INT(OZ_OK, oz_slab_alloc(&small_slab, &b));
	TEST_ASSERT_EQUAL_INT(OZ_ENOMEM, oz_slab_alloc(&small_slab, &c));
	TEST_ASSERT_NULL(c);

	oz_slab_free(&small_slab, b);
	oz_slab_free(&small_slab, a);
}

void test_slab_reuse_after_free(void)
{
	OZ_SLAB_DEFINE(reuse_slab, 32, 1, 4);
	void *first = NULL;
	void *second = NULL;

	TEST_ASSERT_EQUAL_INT(OZ_OK, oz_slab_alloc(&reuse_slab, &first));
	oz_slab_free(&reuse_slab, first);

	/* After free, should be able to alloc again */
	TEST_ASSERT_EQUAL_INT(OZ_OK, oz_slab_alloc(&reuse_slab, &second));
	TEST_ASSERT_NOT_NULL(second);
	oz_slab_free(&reuse_slab, second);
}

void test_slab_free_at_zero_safe(void)
{
	OZ_SLAB_DEFINE(zero_slab, 16, 2, 4);
	void *mem = NULL;
	oz_slab_alloc(&zero_slab, &mem);
	oz_slab_free(&zero_slab, mem);

	/* num_used is 0; free should not underflow */
	TEST_ASSERT_EQUAL_UINT32(0, zero_slab.num_used);
}
