#import "OZTestBase.h"

@interface Item : OZObject
@property(nonatomic, assign) int tag;
@end

@implementation Item
@synthesize tag = _tag;
@end

@interface Holder : OZObject
@property(nonatomic, strong) Item *item;
@property(nonatomic, assign) int value;
@end

@implementation Holder
@synthesize item = _item;
@synthesize value = _value;
@end
