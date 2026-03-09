#import "OZTestBase.h"

@interface Speaker : OZObject {
	int _spoken;
}
- (void)speak;
- (int)spoken;
@end

@implementation Speaker
- (void)speak { _spoken = 1; }
- (int)spoken { return _spoken; }
@end
