/* oz-pool: SyncCounter=1,OZSpinLock=1 */
#import "OZTestBase.h"

@interface SyncCounter : OZObject {
	int _count;
}
- (void)increment;
- (int)count;
@end

@implementation SyncCounter
- (void)increment {
	@synchronized(self) {
		_count = _count + 1;
	}
}
- (int)count {
	return _count;
}
@end
