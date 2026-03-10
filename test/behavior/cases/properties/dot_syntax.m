#import "OZTestBase.h"

@interface Setting : OZObject
@property(nonatomic, assign) int brightness;
@end

@implementation Setting
@synthesize brightness = _brightness;
@end
