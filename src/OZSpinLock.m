/* RAII spinlock implementation for @synchronized support. */

#import <Foundation/OZSpinLock.h>

@implementation OZSpinLock
- (instancetype)initWithObject:(id)obj
{
	_obj = obj;
	return self;
}
- (void)dealloc
{
}
@end
