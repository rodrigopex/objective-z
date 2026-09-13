/* Census fixture: an immortal singleton is not a leak (#451).
 *
 * The one honest complication the issue names. A class conforming to
 * OZSingletonProtocol has every instance marked `_meta.immortal` at alloc
 * and `oz_release` returns before the decrement, so its slab slot is held
 * for the life of the program by design. Counting it would report
 * px-keyboard's four singletons as four leaks.
 *
 * Both halves are asserted here, because "the census said zero" is
 * worthless on its own -- it is also what a census over an *empty* slab
 * says, and what a census nothing calls says:
 *
 *   - presence: the slot really is still outstanding at the end of the
 *     run (`oz_slab_outstanding_count` == 1);
 *   - absence: `oz_check_all_slabs()` nevertheless answers 0, and
 *     `tests/behavior/test_census.py` checks the process exits 0 with no
 *     `LEAK:` line.
 *
 * `oz_slab_Config` is declared here rather than included: OZ_SLAB_DEFINE
 * lands in the class's own generated `.c`, and the generated header does
 * not export it. A driver is hand-written C, so it may say so itself. */
#include "unity.h"
#include "Config_ozh.h"

extern oz_slab_t oz_slab_Config;

void test_the_singleton_holds_its_slab_slot(void)
{
	struct Config *c = Config_cls_sharedInstance();

	TEST_ASSERT_NOT_NULL(c);
	TEST_ASSERT_EQUAL_INT(60, Config_refreshRate(c));

	/* Presence: the slot is out, and stays out. Without this the zero
	 * the census reports would be indistinguishable from an empty
	 * slab. */
	TEST_ASSERT_EQUAL_UINT32(1, oz_slab_outstanding_count(&oz_slab_Config));

	/* Absence, at the same instant: the immortal instance is excluded
	 * from the count even though the slot above is outstanding. */
	TEST_ASSERT_EQUAL_INT(0, oz_check_all_slabs());
}
