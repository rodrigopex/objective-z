#import "OZTestBase.h"

@interface Widget : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation Widget
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
@end
