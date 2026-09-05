/* oz-pool: OZObject=1,OZString=3,OZArray=1,TypedIterTest=1 */
#import "OZFoundationBase.h"

@interface TypedIterTest : OZObject {
	size_t _count;
}
- (void)countStrings;
- (size_t)count;
@end

@implementation TypedIterTest
- (void)countStrings {
	OZArray *arr = @[@"hello", @"world", @"oz"];
	_count = 0;
	for (OZString *s in arr) {
		if ([s length] > 0) {
			_count = _count + 1;
		}
	}
}
- (size_t)count {
	return _count;
}
@end
