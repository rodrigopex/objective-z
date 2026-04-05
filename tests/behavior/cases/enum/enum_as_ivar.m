/* oz-pool: EnumIvarTest=1 */
#import "OZTestBase.h"

enum Direction {
	DirectionNorth = 0,
	DirectionSouth = 1,
	DirectionEast = 2,
	DirectionWest = 3
};

@interface EnumIvarTest : OZObject {
	enum Direction _dir;
}
- (void)setDirection:(enum Direction)d;
- (enum Direction)direction;
@end

@implementation EnumIvarTest
- (void)setDirection:(enum Direction)d {
	_dir = d;
}
- (enum Direction)direction {
	return _dir;
}
@end
