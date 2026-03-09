#import "OZTestBase.h"

@protocol Measurable
- (int)measure;
@end

@interface Ruler : OZObject <Measurable>
@end

@implementation Ruler
- (int)measure { return 30; }
@end

@interface Scale : OZObject <Measurable>
@end

@implementation Scale
- (int)measure { return 100; }
@end
