/* Behavior test: the first release path -- dealloc runs, the slot goes back,
 * and nothing crashes.
 *
 * **This case deliberately performs ONE release, and must not be "fixed"
 * into two.** That looks like the obvious improvement, given the name, and it
 * was tried and measured under #452. It does not work, for a reason that is
 * about the guard itself rather than about this harness:
 *
 * The rc<=0 guard in `oz_release` is only reachable on an object whose
 * storage has already been returned. Reaching it means reading `_meta` and
 * `oz_refcount` out of freed memory, so the unflagged "survival" is
 * undefined behaviour that happens to work, not a defined path. gcc/clang
 * with `-fsanitize=address` says so exactly:
 *
 *     ERROR: AddressSanitizer: heap-use-after-free
 *     READ of size 4 ... in oz_release
 *     freed by thread T0 here: ... oz_slab_free <- Item_oz_free
 *
 * and CI's `sanitizers` job runs this whole corpus under
 * `--sanitize=address,undefined`, with no per-case opt-out. A genuine double
 * release here is therefore a red required check, not a stronger test. An
 * immortal object would survive a second release legitimately, but immortals
 * return at the `_meta.immortal` check *above* the guard, so that exercises a
 * different branch and never reaches this one.
 *
 * Where the double release IS pinned, both halves of it:
 *
 *   - `tools/oz2c/tests/refcount_traps.rs`,
 *     `without_the_flag_a_double_release_is_survived_silently` -- the
 *     unflagged path, in the Rust harness, which does not sanitize.
 *   - the same file, `an_over_release_aborts_rather_than_returning_silently`
 *     -- the trap actually firing under `-DOZ_DEBUG_REFCOUNT`, through
 *     `common::expect_trap`, which requires a non-zero exit.
 *
 * Unity cannot make the second assertion at all: a firing trap calls
 * `oz_assert_msg`, which aborts, and there is no way to catch an abort here
 * -- the binary would die and be reported as a crash rather than a pass. So
 * this case does not test the trap, and does not imply that it does. Its
 * claim is the narrow one below.
 */
#include "unity.h"
#include "Item_ozh.h"

void test_double_release_no_crash(void)
{
	struct Item *item = Item_alloc();
	TEST_ASSERT_NOT_NULL(item);

	/* rc 1 -> 0: runs -dealloc and returns the slot to the slab. The
	 * claim is that this path is clean -- under ASan and UBSan too,
	 * which is what makes it worth a row here rather than only in the
	 * Rust suite. */
	OZObject_release((struct OZObject *)item);

	TEST_PASS();
}
