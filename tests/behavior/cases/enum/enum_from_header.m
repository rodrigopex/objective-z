/* oz-pool: EnumHeaderTest=1 */
#import "OZTestBase.h"

enum Priority {
	PriorityLow = 1,
	PriorityMedium = 5,
	PriorityHigh = 10
};

@interface EnumHeaderTest : OZObject {
	enum Priority _prio;
}
- (void)setPriority:(enum Priority)p;
- (BOOL)isHighPriority;
@end

@implementation EnumHeaderTest
- (void)setPriority:(enum Priority)p {
	_prio = p;
}
- (BOOL)isHighPriority {
	return _prio >= PriorityHigh;
}
@end
