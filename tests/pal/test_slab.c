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

/* ------------------------------------------------------------------ */
/* Leak detection — the two the census is built on                     */
/* ------------------------------------------------------------------ */

/*
 * `oz_slab_outstanding_count` and `oz_slab_check_leaks` had lived in
 * `include/platform/oz_platform_host.h` with **zero** call sites anywhere
 * in the tree -- tests, src, samples, tools, cmake and the workflows
 * (#451). The tests above reach past both and read `test_slab.num_used`
 * directly, which is why nothing noticed. These two cover the accessors
 * themselves, so the census built on them rests on something tested.
 */

void test_slab_outstanding_count_follows_the_slab(void)
{
	OZ_SLAB_DEFINE(census_slab, 16, 2, 4);
	void *a = NULL;

	TEST_ASSERT_EQUAL_UINT32(0, oz_slab_outstanding_count(&census_slab));
	oz_slab_alloc(&census_slab, &a);
	TEST_ASSERT_EQUAL_UINT32(1, oz_slab_outstanding_count(&census_slab));
	oz_slab_free(&census_slab, a);
	TEST_ASSERT_EQUAL_UINT32(0, oz_slab_outstanding_count(&census_slab));
}

void test_slab_check_leaks_answers_both_ways(void)
{
	OZ_SLAB_DEFINE(leak_slab, 16, 2, 4);
	void *a = NULL;

	/* Absence first, then presence: a 0 from `check_leaks` is also what
	 * a broken implementation that always returns 0 would say, so the
	 * clean answer is only worth having next to a dirty one. */
	TEST_ASSERT_EQUAL_INT(0, oz_slab_check_leaks(&leak_slab, "leak_slab"));

	oz_slab_alloc(&leak_slab, &a);
	/* This prints `LEAK: leak_slab has 1 outstanding allocation(s)` on
	 * stderr. That line is this case's expected output, not a failure. */
	TEST_ASSERT_EQUAL_INT(1, oz_slab_check_leaks(&leak_slab, "leak_slab"));

	oz_slab_free(&leak_slab, a);
	TEST_ASSERT_EQUAL_INT(0, oz_slab_check_leaks(&leak_slab, "leak_slab"));
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
