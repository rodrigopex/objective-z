/* oz-pool: OZObject=1,OZQ31=6,OZArray=3,NestedIterTest=1 */
#import "OZFoundationBase.h"

@interface NestedIterTest : OZObject {
	int _total;
}
- (void)nestedIteration;
- (int)total;
@end

@implementation NestedIterTest
- (void)nestedIteration {
	OZArray *outer = @[@(1), @(2)];
	OZArray *inner = @[@(10), @(20)];
	_total = 0;
	for (OZQ31 *a in outer) {
		for (OZQ31 *b in inner) {
			_total = _total + [a intValue] + [b intValue];
		}
	}
}
- (int)total {
	return _total;
}
@end
