/*
 * Behavioral spec derived from: Apple objc4 property documentation
 * This test is ORIGINAL CODE — no Apple code was copied.
 * Pattern: readonly enforcement, strong vs assign semantics.
 */
/* oz-pool: PropAttrTest=1 */
#import "OZTestBase.h"

@interface PropAttrTest : OZObject {
	int _readwrite;
	int _readonly;
}
@property (nonatomic, assign) int readwrite;
@property (nonatomic, readonly) int readonly;
- (void)setReadonlyDirect:(int)v;
@end

@implementation PropAttrTest
@synthesize readwrite = _readwrite;
@synthesize readonly = _readonly;
- (void)setReadonlyDirect:(int)v {
	_readonly = v;
}
@end
