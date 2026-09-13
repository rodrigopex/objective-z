/* SPDX-License-Identifier: Apache-2.0 */
/*
 * Census tests: the on-target half of the exit-time live-object census
 * (#451).
 *
 * On target this is the *only* leak instrument there is. `k_mem_slab` is
 * static memory, so an unreleased object is invisible by construction --
 * nothing is allocated from a system heap, nothing is unreachable, and no
 * sanitizer runs. Before this, no sample and no ztest queried
 * `k_mem_slab_num_used_get` at all, and the Zephyr PAL had neither
 * `oz_slab_outstanding_count` nor `oz_slab_check_leaks`; only the host
 * backend did, and there with zero call sites.
 *
 * The first two cases pin the two PAL accessors against a slab this file
 * owns, so they answer for themselves regardless of what the rest of the
 * suites did. The third asks the whole-program question the generated
 * `oz_check_all_slabs()` exists to answer, and is deliberately
 * order-independent: every other suite here balances its allocations, so
 * a non-zero answer is a real finding about one of them rather than a
 * side effect of which suite ran first.
 */
#include <zephyr/ztest.h>
#include "Widget_ozh.h"
#include "OZObject_ozh.h"
#include "oz_dispatch.h"

/* File scope, because on this backend OZ_SLAB_DEFINE is
 * K_MEM_SLAB_DEFINE and reserves static storage plus its own buffer. */
OZ_SLAB_DEFINE(census_slab, 32, 2, 4);

ZTEST_SUITE(census, NULL, NULL, NULL, NULL, NULL);

ZTEST(census, test_outstanding_count_follows_the_slab)
{
	void *a = NULL;

	zassert_equal(0u, oz_slab_outstanding_count(&census_slab));
	zassert_equal(OZ_OK, oz_slab_alloc(&census_slab, &a));
	zassert_equal(1u, oz_slab_outstanding_count(&census_slab));
	oz_slab_free(&census_slab, a);
	zassert_equal(0u, oz_slab_outstanding_count(&census_slab));
}

ZTEST(census, test_check_leaks_answers_both_ways)
{
	void *a = NULL;

	/* Absence first, then presence. A 0 on its own is also what an
	 * implementation that always returns 0 would say, so the clean
	 * answer is only worth having next to a dirty one. */
	zassert_equal(0, oz_slab_check_leaks(&census_slab, "census_slab"));

	zassert_equal(OZ_OK, oz_slab_alloc(&census_slab, &a));
	/* Prints `LEAK: census_slab has 1 outstanding allocation(s)` on the
	 * console. That line is this case's expected output. */
	zassert_equal(1, oz_slab_check_leaks(&census_slab, "census_slab"));

	oz_slab_free(&census_slab, a);
	zassert_equal(0, oz_slab_check_leaks(&census_slab, "census_slab"));
}

ZTEST(census, test_the_program_census_is_clean)
{
	/* Presence: this suite can make the census dirty, so the clean
	 * answer below is a measurement rather than a property of a census
	 * that cannot see anything. */
	struct Widget *w = Widget_alloc();

	zassert_not_null(w);
	zassert_equal(1, oz_check_all_slabs(),
		      "an outstanding Widget must be counted");

	OZObject_release((struct OZObject *)w);

	/* Absence: with it released, every slab in the program is empty. */
	zassert_equal(0, oz_check_all_slabs(),
		      "every slab block handed out must have been handed back");
}
