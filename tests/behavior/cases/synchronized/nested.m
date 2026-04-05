/* oz-pool: NestLock=2,OZSpinLock=2 */
#import "OZTestBase.h"

@interface NestLock : OZObject {
	int _outer;
	int _inner;
}
- (void)runNested:(NestLock *)other;
- (int)outer;
- (int)inner;
@end

@implementation NestLock
- (void)runNested:(NestLock *)other {
	@synchronized(self) {
		_outer = 1;
		@synchronized(other) {
			_inner = 2;
		}
	}
}
- (int)outer {
	return _outer;
}
- (int)inner {
	return _inner;
}
@end
