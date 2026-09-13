/*
 * Behavioral spec derived from: mulle-objc runtime lifecycle patterns
 * mulle-objc license: BSD-3-Clause
 * This test is ORIGINAL CODE inspired by mulle-objc's lifecycle conventions.
 * Pattern: ARC scope-exit frees slab; re-alloc proves lifecycle works.
 */
/* oz-pool: BalanceObj=1,BalanceTest=1 */
#import "OZTestBase.h"

@interface BalanceObj : OZObject
@end

@implementation BalanceObj
@end

@interface BalanceTest : OZObject {
	int _allocOk;
	int _poolIsOne;
	int _reuseOk;
}
- (void)run;
- (int)allocOk;
- (int)poolIsOne;
- (int)reuseOk;
@end

@implementation BalanceTest
/*
 * The pool is one slot, and that is what makes this a balance test rather
 * than two non-NULL checks.
 *
 * `_reuseOk` alone proves nothing: on a two-slot pool it would pass with
 * no release happening at all, which is the shape #455 was filed about --
 * a test named "retain release balance" whose assertions cannot fail for
 * the reason the test exists. `_poolIsOne` is the control that gives it
 * its meaning: while the first object is alive the slab is full, so a
 * second alloc must come back nil. Only then does a *later* alloc
 * succeeding say that ARC released the first at scope exit.
 */
- (void)run {
	{
		BalanceObj *obj = [BalanceObj alloc];
		_allocOk = (obj != nil) ? 1 : 0;

		/* Full slab, so this must fail -- the control. */
		BalanceObj *denied = [BalanceObj alloc];
		_poolIsOne = (denied == nil) ? 1 : 0;
	}
	/* ARC scope-exit frees the slot — re-alloc proves the release ran */
	BalanceObj *obj2 = [BalanceObj alloc];
	_reuseOk = (obj2 != nil) ? 1 : 0;
}
- (int)allocOk {
	return _allocOk;
}
- (int)poolIsOne {
	return _poolIsOne;
}
- (int)reuseOk {
	return _reuseOk;
}
@end
