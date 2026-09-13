/* oz-pool: Config=1 */
#import "OZTestBase.h"
#import <Foundation/OZSingletonProtocol.h>

@interface Config : OZObject <OZSingletonProtocol> {
	int _refreshRate;
}
+ (instancetype)sharedInstance;
- (int)refreshRate;
@end

/* File-scope static: the shape LSan is structurally blind to. The block
 * is reachable from a root for the whole run, so `-fsanitize=leak` has
 * nothing to report about it either way -- which is why the census
 * counts allocations rather than reachability. */
static Config *sSharedConfig;

@implementation Config
+ (void)initialize
{
	sSharedConfig = [[Config alloc] init];
}
+ (instancetype)sharedInstance
{
	return sSharedConfig;
}
- (id)init
{
	self = [super init];
	_refreshRate = 60;
	return self;
}
- (int)refreshRate
{
	return _refreshRate;
}
@end
