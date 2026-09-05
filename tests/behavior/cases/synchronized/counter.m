/* oz-pool: SyncCounter=1,OZSpinLock=1 */
#import "OZTestBase.h"

@interface SyncCounter : OZObject {
	size_t _count;
}
- (void)increment;
- (size_t)count;
@end

@implementation SyncCounter
- (void)increment {
	@synchronized(self) {
		_count = _count + 1;
	}
}
- (size_t)count {
	return _count;
}
@end
