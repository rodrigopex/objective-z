/*
 * Adapted from: tests/objc-reference/runtime/flat_dispatch/src/main.c
 * Adaptation: Removed objc_msg_lookup introspection.
 *             Replaced with direct calls at each hierarchy level.
 *             Pattern: multi-level hierarchy dispatch resolves correctly.
 */
#import "OZTestBase.h"

@interface Base : OZObject
- (int)level;
@end

@implementation Base
- (int)level { return 1; }
@end

@interface Mid : Base
- (int)level;
@end

@implementation Mid
- (int)level { return 2; }
@end

@interface Tip : Mid
- (int)level;
@end

@implementation Tip
- (int)level { return 3; }
@end
