/* oz-pool: StaticBlockTest=1 */
#import "OZTestBase.h"

static int g_multiplier = 3;

@interface StaticBlockTest : OZObject {
	int _result;
}
- (void)run;
- (int)result;
@end

@implementation StaticBlockTest
- (void)run {
	int (^mul)(int) = ^(int x) {
		return x * g_multiplier;
	};
	_result = mul(5);
}
- (int)result {
	return _result;
}
@end
