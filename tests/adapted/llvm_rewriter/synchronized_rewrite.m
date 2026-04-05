/*
 * Adapted from: clang/test/Rewriter/objc-synchronized-1.m
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies @synchronized lowers to OZSpinLock RAII correctly.
 */
/* oz-pool: SyncObj=1,OZSpinLock=1 */
#import "OZTestBase.h"

@interface SyncObj : OZObject {
	int _counter;
}
- (void)increment;
- (int)counter;
@end

@implementation SyncObj
- (void)increment {
	@synchronized(self) {
		_counter = _counter + 1;
	}
}
- (int)counter {
	return _counter;
}
@end
