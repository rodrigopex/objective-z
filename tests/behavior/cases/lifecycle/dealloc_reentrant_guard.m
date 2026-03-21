#import "OZTestBase.h"

@interface Probe : OZObject
@end

@implementation Probe

- (void)dealloc
{
	/* Classic Objective-C pattern: retain+release self during dealloc.
	 * Without the deallocating guard, the release would trigger
	 * another dealloc -> infinite recursion. */
	[self retain];
	[self release];
}

@end
