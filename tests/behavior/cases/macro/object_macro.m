/* oz-pool: MacroObjTest=1 */
#import "OZTestBase.h"

#define MAX_COUNT 10

@interface MacroObjTest : OZObject {
	int _value;
}
- (void)run;
- (int)value;
@end

@implementation MacroObjTest
- (void)run {
	_value = MAX_COUNT;
}
- (int)value {
	return _value;
}
@end
