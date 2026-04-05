/*
 * Behavioral spec derived from: Apple objc4/test/arr-weak.m
 * This test is ORIGINAL CODE — no Apple code was copied.
 * Pattern: object dealloc frees slab, verifying cleanup.
 * Note: OZ does not support __weak yet — tests slab cleanup instead.
 */
/* oz-pool: Transient=1,WeakTest=1 */
#import "OZTestBase.h"

@interface Transient : OZObject {
	int _val;
}
- (void)setVal:(int)v;
- (int)val;
@end

@implementation Transient
- (void)setVal:(int)v { _val = v; }
- (int)val { return _val; }
@end

@interface WeakTest : OZObject {
	int _reclaimOk;
}
- (void)run;
- (int)reclaimOk;
@end

@implementation WeakTest
- (void)run {
	{
		Transient *t = [Transient alloc];
		[t setVal:42];
	}
	/* ARC scope-exit reclaims slab */
	Transient *proof = [Transient alloc];
	_reclaimOk = (proof != nil) ? 1 : 0;
}
- (int)reclaimOk {
	return _reclaimOk;
}
@end
