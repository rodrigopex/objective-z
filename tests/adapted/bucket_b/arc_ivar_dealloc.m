/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 * Adaptation: Removed __objc_refcount_get introspection.
 *             Replaced with slab-reuse verification.
 *             Pattern: object ivars released during dealloc.
 */
/* oz-pool: Child=1,Parent=1,IvarDeallocTest=1 */
#import "OZTestBase.h"

@interface Child : OZObject
@end

@implementation Child
@end

@interface Parent : OZObject {
	Child *_child;
}
- (void)setChild:(Child *)c;
@end

@implementation Parent
- (void)setChild:(Child *)c {
	_child = c;
}
@end

@interface IvarDeallocTest : OZObject {
	int _canReallocChild;
}
- (void)run;
- (int)canReallocChild;
@end

@implementation IvarDeallocTest
- (void)run {
	{
		Parent *p = [Parent alloc];
		Child *c = [Child alloc];
		[p setChild:c];
	}
	/* ARC scope-exit releases both — child slab should be free */
	Child *proof = [Child alloc];
	_canReallocChild = (proof != nil) ? 1 : 0;
}
- (int)canReallocChild {
	return _canReallocChild;
}
@end
