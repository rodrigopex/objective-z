/*
 * Adapted from: clang/test/Rewriter method rewriting
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies multi-argument selectors are correctly mangled
 *             into C function names (e.g. -[Calc add:to:] -> Calc_add_to_).
 */
#import "OZTestBase.h"

@interface Calc : OZObject
- (int)add:(int)a to:(int)b;
- (int)multiply:(int)a by:(int)b offset:(int)c;
@end

@implementation Calc
- (int)add:(int)a to:(int)b { return a + b; }
- (int)multiply:(int)a by:(int)b offset:(int)c { return a * b + c; }
@end
