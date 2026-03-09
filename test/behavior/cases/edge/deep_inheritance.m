#import "OZTestBase.h"

@interface Level1 : OZObject
- (int)depth;
@end

@implementation Level1
- (int)depth { return 1; }
@end

@interface Level2 : Level1
- (int)depth;
@end

@implementation Level2
- (int)depth { return 2; }
@end

@interface Level3 : Level2
- (int)depth;
@end

@implementation Level3
- (int)depth { return 3; }
@end

@interface Level4 : Level3
- (int)depth;
@end

@implementation Level4
- (int)depth { return 4; }
@end
