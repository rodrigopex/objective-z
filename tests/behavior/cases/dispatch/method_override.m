#import "OZTestBase.h"

@interface Animal : OZObject
- (int)sound;
@end

@implementation Animal
- (int)sound { return 1; }
@end

@interface Dog : Animal
- (int)sound;
@end

@implementation Dog
- (int)sound { return 2; }
@end
