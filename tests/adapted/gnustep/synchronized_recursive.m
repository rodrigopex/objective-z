/*
 * Adapted from: GNUstep libobjc2 — Test/Synchronized.m
 * License: MIT
 * Adaptation: Verifies recursive @synchronized on same object doesn't deadlock.
 */
/* oz-pool: RecursiveSyncTest=1,OZSpinLock=2 */
#import "OZTestBase.h"

@interface RecursiveSyncTest : OZObject {
	int _depth;
}
- (void)recursiveLock;
- (int)depth;
@end

@implementation RecursiveSyncTest
- (void)recursiveLock {
	@synchronized(self) {
		_depth = 1;
		@synchronized(self) {
			_depth = 2;
		}
	}
}
- (int)depth {
	return _depth;
}
@end
