/**
 * @file OZArray.h
 * @brief Immutable array class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits pure-C static array constants.
 */
#pragma once
#import "OZObject.h"

@interface OZArray : OZObject {
	id *_items;
	unsigned int _count;
}
+ (id)arrayWithObjects:(const id *)objects count:(unsigned int)count;
- (unsigned int)count;
- (id)objectAtIndex:(unsigned int)index;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end

@compatibility_alias NSArray OZArray;
