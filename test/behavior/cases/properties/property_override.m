#import "OZTestBase.h"

@interface Shape : OZObject {
	int _sides;
}
- (int)sides;
@end

@implementation Shape
- (int)sides { return _sides; }
@end

@interface Square : Shape
- (int)sides;
@end

@implementation Square
- (int)sides { return 4; }
@end
