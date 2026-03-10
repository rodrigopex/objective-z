#import "OZTestBase.h"

@interface Counter : OZObject
@property(assign) int count;
@end

@implementation Counter
@synthesize count = _count;
@end

@interface Container : OZObject
@property(strong) Counter *counter;
@end

@implementation Container
@synthesize counter = _counter;
@end
