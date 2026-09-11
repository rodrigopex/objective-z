/* oz-pool: OZObject=1,OZNumber=4,OZArray=1,IterTest=1 */
#import "OZFoundationBase.h"

@interface IterTest : OZObject {
	int _sum;
}
- (void)sumArray;
- (int)sum;
@end

@implementation IterTest
- (void)sumArray {
	OZArray *arr = @[@(10), @(20), @(30)];
	_sum = 0;
	for (OZNumber *n in arr) {
		_sum = _sum + [n intValue];
	}
}
- (int)sum {
	return _sum;
}
@end
