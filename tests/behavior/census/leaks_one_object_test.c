/* Census fixture: one object allocated and never released (#451).
 *
 * Not a corpus case -- this one is *expected* to fail, and it lives
 * outside `cases/` so the corpus never collects it. Its whole job is to
 * prove the exit-time census can fail: the Unity assertion below passes,
 * so a non-zero exit can only have come from `oz_check_all_slabs()`.
 *
 * The pool is one block, which is also what makes the point about the
 * oracle this replaces: the old alloc/release/re-alloc proxy would have
 * had nothing to say here, because nothing re-allocates. */
#include "unity.h"
#include "Leaky_ozh.h"

void test_the_allocation_itself_succeeds(void)
{
	struct Leaky *l = Leaky_alloc();

	TEST_ASSERT_NOT_NULL(l);
	/* Deliberately no release. The slab slot stays out, and `l` is a
	 * local that dies here -- so this is a leak LSan can also see. The
	 * census sees it without a sanitizer, at every -O and compiler. */
}
