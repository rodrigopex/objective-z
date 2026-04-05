/*
 * Adapted from: tests/objc-reference/runtime/arc_intensive/src/main.c
 * Adaptation: Removed objc_stats and __objc_refcount_get introspection.
 *             Replaced with slab-reuse verification.
 *             Pattern: nested retain/release with multiple object locals.
 */
/* oz-pool: Leaf=2,Branch=1,NestedArcTest=1 */
#import "OZTestBase.h"

@interface Leaf : OZObject {
	int _id;
}
- (void)setId:(int)i;
- (int)id;
@end

@implementation Leaf
- (void)setId:(int)i { _id = i; }
- (int)id { return _id; }
@end

@interface Branch : OZObject {
	Leaf *_left;
	Leaf *_right;
}
- (void)setLeft:(Leaf *)l;
- (void)setRight:(Leaf *)r;
- (int)leftId;
- (int)rightId;
@end

@implementation Branch
- (void)setLeft:(Leaf *)l { _left = l; }
- (void)setRight:(Leaf *)r { _right = r; }
- (int)leftId { return [_left id]; }
- (int)rightId { return [_right id]; }
@end

@interface NestedArcTest : OZObject {
	int _leftVal;
	int _rightVal;
}
- (void)run;
- (int)leftVal;
- (int)rightVal;
@end

@implementation NestedArcTest
- (void)run {
	Branch *b = [Branch alloc];
	Leaf *l = [Leaf alloc];
	Leaf *r = [Leaf alloc];
	[l setId:10];
	[r setId:20];
	[b setLeft:l];
	[b setRight:r];
	_leftVal = [b leftId];
	_rightVal = [b rightId];
}
- (int)leftVal { return _leftVal; }
- (int)rightVal { return _rightVal; }
@end
