/* oz-pool: LockTest=1,OZSpinLock=1 */
#import "OZTestBase.h"

@interface LockTest : OZObject {
	int _flag;
}
- (void)run;
- (int)flag;
@end

@implementation LockTest
- (void)run {
	@synchronized(self) {
		_flag = 42;
	}
}
- (int)flag {
	return _flag;
}
@end
