#import "OZTestBase.h"

@interface Factory : OZObject
+ (int)version;
@end

@implementation Factory
+ (int)version { return 42; }
@end
