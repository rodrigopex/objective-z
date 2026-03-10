#import "OZTestBase.h"

@interface Switch : OZObject
@property(nonatomic, assign, getter=isEnabled) BOOL enabled;
@property(nonatomic, assign, setter=applySpeed:) int speed;
@end

@implementation Switch
@synthesize enabled = _on;
@synthesize speed = _spd;
@end
