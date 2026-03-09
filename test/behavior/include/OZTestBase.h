/* SPDX-License-Identifier: Apache-2.0 */
/* Minimal OZObject for behavior test .m files.
 * Parsed by Clang for AST generation — not compiled directly.
 * Must include both @interface and @implementation so the transpiler
 * generates OZObject methods (retain, release, etc.) and dispatch tables. */

#import <stdint.h>

typedef int BOOL;

@interface OZObject
- (instancetype)init;
- (void)dealloc;
- (id)retain;
- (void)release;
- (uint32_t)retainCount;
- (BOOL)isEqual:(id)anObject;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end

@implementation OZObject

- (instancetype)init
{
	return self;
}

- (void)dealloc
{
}

@end
