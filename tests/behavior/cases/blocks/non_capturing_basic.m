/* oz-pool: BlockBasicTest=1 */
#import "OZTestBase.h"

@interface BlockBasicTest : OZObject {
	int _result;
}
- (void)run;
- (int)result;
@end

@implementation BlockBasicTest
- (void)run {
	int (^square)(int) = ^(int x) {
		return x * x;
	};
	_result = square(7);
}
- (int)result {
	return _result;
}
@end
