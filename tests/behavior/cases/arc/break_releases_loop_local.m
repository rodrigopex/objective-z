/* oz-pool: Ephemeral=1,ArcBreakTest=1 */
#import "OZTestBase.h"

@interface Ephemeral : OZObject
@end

@implementation Ephemeral
@end

@interface ArcBreakTest : OZObject {
	int _flag;
}
- (void)run;
- (int)flag;
@end

@implementation ArcBreakTest
- (void)run {
	int i = 0;
	while (i < 3) {
		Ephemeral *t = [Ephemeral alloc];
		if (i == 1) {
			break;
		}
		i = i + 1;
	}
	/* If break released the local, the 1-block slab is free again */
	Ephemeral *proof = [Ephemeral alloc];
	_flag = (proof != nil) ? 1 : 0;
}
- (int)flag {
	return _flag;
}
@end
