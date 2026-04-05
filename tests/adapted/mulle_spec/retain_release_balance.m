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
	int _reuseOk;
}
- (void)run;
- (int)allocOk;
- (int)reuseOk;
@end

@implementation BalanceTest
- (void)run {
	{
		BalanceObj *obj = [BalanceObj alloc];
		_allocOk = (obj != nil) ? 1 : 0;
	}
	/* ARC scope-exit frees slab — re-alloc proves lifecycle works */
	BalanceObj *obj2 = [BalanceObj alloc];
	_reuseOk = (obj2 != nil) ? 1 : 0;
}
- (int)allocOk {
	return _allocOk;
}
- (int)reuseOk {
	return _reuseOk;
}
@end
