/* oz-pool: Gadget=2 */
#import "OZTestBase.h"

@interface Gadget : OZObject {
	int _tag;
}
- (int)tag;
- (void)setTag:(int)tag;
@end

@implementation Gadget
- (int)tag { return _tag; }
- (void)setTag:(int)tag { _tag = tag; }
@end
