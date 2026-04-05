/* oz-pool: Slot=1,ArcReassignTest=1 */
#import "OZTestBase.h"

@interface Slot : OZObject
@end

@implementation Slot
@end

@interface ArcReassignTest : OZObject {
	int _canRealloc;
}
- (void)run;
- (int)canRealloc;
@end

@implementation ArcReassignTest
- (void)run {
	/* 1-block slab: each reassignment must release old before alloc new */
	Slot *s = [Slot alloc];
	s = [Slot alloc];
	s = [Slot alloc];
	/* If old was released on reassign, slab has free blocks */
	_canRealloc = 1;
}
- (int)canRealloc {
	return _canRealloc;
}
@end
