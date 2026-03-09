/*
 * Behavioral spec: nil messaging returns zero for all return types.
 * Based on ObjC language spec (NOT Apple code).
 */
#import "OZTestBase.h"

@interface Widget : OZObject {
	int _tag;
}
- (int)tag;
- (void)setTag:(int)tag;
@end

@implementation Widget
- (int)tag { return _tag; }
- (void)setTag:(int)tag { _tag = tag; }
@end
