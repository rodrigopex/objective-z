#import "OZTestBase.h"

@protocol Readable
- (int)read;
@end

@protocol Writable
- (int)write;
@end

@interface Stream : OZObject <Readable, Writable>
@end

@implementation Stream
- (int)read { return 1; }
- (int)write { return 2; }
@end
