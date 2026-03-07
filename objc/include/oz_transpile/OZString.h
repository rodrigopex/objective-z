/**
 * @file OZString.h
 * @brief Constant string class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits a pure-C struct and static string constants.
 */
#pragma once
#import "OZObject.h"

@interface OZString : OZObject {
	unsigned int _length;
	unsigned int _hash;
	const char *_data;
}
- (const char *)cStr;
- (unsigned int)length;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end
