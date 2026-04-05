/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 * Adaptation: Removed __objc_refcount_get introspection.
 *             Replaced with slab-reuse verification (1-block slab).
 *             Pattern: scope-exit cleanup of local object variables.
 */
/* oz-pool: ScopeObj=1,ScopeTest=1 */
#import "OZTestBase.h"

@interface ScopeObj : OZObject
@end

@implementation ScopeObj
@end

@interface ScopeTest : OZObject {
	int _canRealloc;
}
- (void)testScopeCleanup;
- (int)canRealloc;
@end

@implementation ScopeTest
- (void)testScopeCleanup {
	{
		ScopeObj *local = [ScopeObj alloc];
	}
	/* If scope exit released local, 1-block slab is free */
	ScopeObj *proof = [ScopeObj alloc];
	_canRealloc = (proof != nil) ? 1 : 0;
}
- (int)canRealloc {
	return _canRealloc;
}
@end
