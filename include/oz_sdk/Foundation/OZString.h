/**
 * @file OZString.h
 * @brief Immutable string class for OZ transpiler.
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
- (const char *)cString;
- (unsigned int)length;
- (BOOL)isEqual:(id)anObject;
- (BOOL)isEqualToString:(OZString *)aString;
- (BOOL)hasPrefix:(OZString *)prefix;
- (BOOL)hasSuffix:(OZString *)suffix;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end

#ifdef __clang__
@compatibility_alias NSString OZString;
#endif
