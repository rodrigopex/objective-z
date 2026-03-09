#import "OZTestBase.h"

@interface Dummy : OZObject
- (int)value;
@end

@implementation Dummy
- (int)value { return 42; }
@end
