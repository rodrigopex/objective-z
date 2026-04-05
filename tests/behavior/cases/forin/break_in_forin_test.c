/* Behavior test: break inside for-in stops iteration early */
#include "unity.h"
#include "oz_dispatch.h"
#include "BreakIterTest_ozh.h"

void test_forin_break_stops_early(void)
{
	struct BreakIterTest *t = BreakIterTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	BreakIterTest_breakAtThreshold(t);
	/* Should have processed 1 and 2, then hit break at 3 */
	TEST_ASSERT_EQUAL_INT(2, BreakIterTest_stoppedAt(t));
	OZObject_release((struct OZObject *)t);
}
