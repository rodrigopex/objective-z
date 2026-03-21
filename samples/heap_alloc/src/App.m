#import "App.h"

static App *app;

@implementation App
@synthesize heap = _heap;

+ (void)initialize
{
	app = [[App alloc] init];
}

+ (instancetype)shared
{
	return app;
}

- (id)init
{
	self = [super init];
	if (self != nil) {
		static char appHeapBuffer[2048];
		_heap = [[OZHeap alloc] initWithBuffer:appHeapBuffer
		                                  size:sizeof(appHeapBuffer)];
		OZLog("App heap initialized (%d bytes)", (int)sizeof(appHeapBuffer));
	}
	return self;
}

@end
