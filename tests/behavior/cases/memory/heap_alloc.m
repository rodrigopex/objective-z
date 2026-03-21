/* oz-heap */
#import <Foundation/OZObject.h>
#import <Foundation/OZHeap.h>
#import "OZObject.m"
#import "OZHeap.m"

@interface Widget : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation Widget

- (id)init
{
	self = [super init];
	return self;
}

- (void)setTag:(int)t
{
	_tag = t;
}

- (int)tag
{
	return _tag;
}

- (void)dealloc
{
}

@end
