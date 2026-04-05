/* oz-pool: OZObject=1,OZQ31=4,OZArray=1,BreakIterTest=1 */
#import "OZFoundationBase.h"

@interface BreakIterTest : OZObject {
	int _stoppedAt;
}
- (void)breakAtThreshold;
- (int)stoppedAt;
@end

@implementation BreakIterTest
- (void)breakAtThreshold {
	OZArray *arr = @[@(1), @(2), @(3), @(4)];
	_stoppedAt = 0;
	for (OZQ31 *n in arr) {
		int v = [n intValue];
		if (v == 3) {
			break;
		}
		_stoppedAt = v;
	}
}
- (int)stoppedAt {
	return _stoppedAt;
}
@end
