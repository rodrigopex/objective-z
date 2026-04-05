/* oz-pool: EnumSwitchTest=1 */
#import "OZTestBase.h"

enum Color {
	ColorRed = 0,
	ColorGreen = 1,
	ColorBlue = 2
};

@interface EnumSwitchTest : OZObject {
	int _result;
}
- (void)classifyColor:(enum Color)c;
- (int)result;
@end

@implementation EnumSwitchTest
- (void)classifyColor:(enum Color)c {
	switch (c) {
		case ColorRed:
			_result = 10;
			break;
		case ColorGreen:
			_result = 20;
			break;
		case ColorBlue:
			_result = 30;
			break;
	}
}
- (int)result {
	return _result;
}
@end
