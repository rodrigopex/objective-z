#import "OZTestBase.h"

@interface Holder : OZObject {
	int _value;
}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Holder
- (void)setValue:(int)v { _value = v; }
- (int)value { return _value; }
@end
