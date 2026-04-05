/* oz-pool: EarlyRet=1,OZSpinLock=1 */
#import "OZTestBase.h"

@interface EarlyRet : OZObject {
	int _value;
}
- (int)compute;
- (int)value;
@end

@implementation EarlyRet
- (int)compute {
	@synchronized(self) {
		_value = 77;
		return _value;
	}
}
- (int)value {
	return _value;
}
@end
