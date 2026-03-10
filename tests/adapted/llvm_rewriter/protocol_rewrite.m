/*
 * Adapted from: clang/test/Rewriter protocol handling
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies protocol declaration produces the
 *             OZ_PROTOCOL_SEND switch dispatch function.
 */
#import "OZTestBase.h"

@protocol Drawable
- (int)draw;
@end

@interface Circle : OZObject <Drawable> {
	int _radius;
}
@end

@implementation Circle
- (int)draw { return _radius * 2; }
@end

@interface Square : OZObject <Drawable> {
	int _side;
}
- (void)setSide:(int)side;
@end

@implementation Square
- (int)draw { return _side * 4; }
- (void)setSide:(int)side { _side = side; }
@end
