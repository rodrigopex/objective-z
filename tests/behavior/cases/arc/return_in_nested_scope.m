/* oz-pool: Inner=1,ArcReturnTest=1 */
#import "OZTestBase.h"

@interface Inner : OZObject
@end

@implementation Inner
@end

@interface ArcReturnTest : OZObject {
	int _result;
}
- (int)earlyReturnTest;
- (int)result;
@end

@implementation ArcReturnTest
- (int)earlyReturnTest {
	int i = 0;
	while (i < 3) {
		Inner *obj = [Inner alloc];
		if (i == 1) {
			return 42;
		}
		i = i + 1;
	}
	return -1;
}
- (int)result {
	return _result;
}
@end
