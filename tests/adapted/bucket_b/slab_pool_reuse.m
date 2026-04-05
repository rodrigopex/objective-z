/*
 * Adapted from: tests/objc-reference/runtime/static_pools/src/main.c
 * Adaptation: No introspection to remove — static pools are direct.
 *             Pattern: allocate, free, re-allocate from same slab.
 */
/* oz-pool: Pooled=1,PoolReuseTest=1 */
#import "OZTestBase.h"

@interface Pooled : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation Pooled
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
@end

@interface PoolReuseTest : OZObject {
	int _reuseOk;
}
- (void)run;
- (int)reuseOk;
@end

@implementation PoolReuseTest
- (void)run {
	{
		Pooled *a = [Pooled alloc];
		[a setTag:42];
	}
	/* ARC scope-exit returns slab block — re-alloc must succeed */
	Pooled *b = [Pooled alloc];
	_reuseOk = (b != nil) ? 1 : 0;
}
- (int)reuseOk {
	return _reuseOk;
}
@end
