#import "OZTestBase.h"

@interface Config : OZObject
@property(nonatomic, assign) int level;
@end

@implementation Config
@synthesize level = _level;
@end
