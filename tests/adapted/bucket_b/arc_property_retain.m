/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 * Adaptation: Removed objc_getProperty/objc_setProperty introspection.
 *             Replaced with direct property access and slab reuse.
 *             Pattern: property setter retains new value.
 */
/* oz-pool: Held=2,PropHolder=1 */
#import "OZTestBase.h"

@interface Held : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation Held
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
@end

@interface PropHolder : OZObject {
	Held *_item;
}
@property (nonatomic, strong) Held *item;
- (int)itemTag;
@end

@implementation PropHolder
@synthesize item = _item;
- (int)itemTag {
	return [_item tag];
}
@end
