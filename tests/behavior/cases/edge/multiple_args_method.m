#import "OZTestBase.h"

@interface Calc : OZObject
- (int)addA:(int)a b:(int)b c:(int)c;
@end

@implementation Calc
- (int)addA:(int)a b:(int)b c:(int)c { return a + b + c; }
@end
