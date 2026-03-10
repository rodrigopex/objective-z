#import "OZTestBase.h"

@protocol Runnable
- (int)run;
@end

@protocol FastRunnable <Runnable>
- (int)sprint;
@end

@interface Athlete : OZObject <FastRunnable>
@end

@implementation Athlete
- (int)run { return 5; }
- (int)sprint { return 10; }
@end
