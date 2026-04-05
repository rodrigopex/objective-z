/* oz-pool: MacroFuncTest=1 */
#import "OZTestBase.h"

#define DOUBLE(x) ((x) * 2)

@interface MacroFuncTest : OZObject {
	int _doubled;
}
- (void)runWithValue:(int)v;
- (int)doubled;
@end

@implementation MacroFuncTest
- (void)runWithValue:(int)v {
	_doubled = DOUBLE(v);
}
- (int)doubled {
	return _doubled;
}
@end
