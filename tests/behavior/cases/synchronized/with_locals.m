/* oz-pool: SyncLocal=2,OZSpinLock=1 */
#import "OZTestBase.h"

@interface SyncLocal : OZObject {
	int _marker;
}
- (void)run;
- (int)marker;
@end

@implementation SyncLocal
- (void)run {
	@synchronized(self) {
		SyncLocal *tmp = [SyncLocal alloc];
		_marker = 1;
	}
}
- (int)marker {
	return _marker;
}
@end
