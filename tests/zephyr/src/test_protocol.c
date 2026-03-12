/* SPDX-License-Identifier: Apache-2.0 */
/* Protocol dispatch tests: vtable routing to correct class */
#include <zephyr/ztest.h>
#include "LightSwitch_ozh.h"
#include "Fan_ozh.h"
#include "OZObject_ozh.h"
#include "oz_dispatch.h"

ZTEST_SUITE(protocol, NULL, NULL, NULL, NULL, NULL);

ZTEST(protocol, test_toggle_routes_to_lightswitch)
{
	struct LightSwitch *ls = LightSwitch_alloc();
	int result = OZ_SEND_toggle((struct OZObject *)ls);
	zassert_equal(1, result, "LightSwitch toggle should return 1");
	OZObject_release((struct OZObject *)ls);
}

ZTEST(protocol, test_toggle_routes_to_fan)
{
	struct Fan *f = Fan_alloc();
	int result = OZ_SEND_toggle((struct OZObject *)f);
	zassert_equal(11, result, "Fan toggle should return 11");
	OZObject_release((struct OZObject *)f);
}

ZTEST(protocol, test_toggle_is_stateful)
{
	struct LightSwitch *ls = LightSwitch_alloc();

	/* First toggle: 0 -> 1 */
	int r1 = OZ_SEND_toggle((struct OZObject *)ls);
	zassert_equal(1, r1, "First toggle should return 1");

	/* Second toggle: 1 -> 0 */
	int r2 = OZ_SEND_toggle((struct OZObject *)ls);
	zassert_equal(0, r2, "Second toggle should return 0");

	OZObject_release((struct OZObject *)ls);
}
