/* oz-pool: Token=1,ArcContinueTest=1 */
#import "OZTestBase.h"

@interface Token : OZObject
@end

@implementation Token
@end

@interface ArcContinueTest : OZObject {
	int _iterations;
}
- (void)run;
- (int)iterations;
@end

@implementation ArcContinueTest
- (void)run {
	_iterations = 0;
	int i = 0;
	while (i < 3) {
		Token *t = [Token alloc];
		i = i + 1;
		_iterations = _iterations + 1;
		continue;
	}
	/* If continue released locals each iteration, slab is free */
	Token *proof = [Token alloc];
	_iterations = (proof != nil) ? _iterations : -1;
}
- (int)iterations {
	return _iterations;
}
@end
