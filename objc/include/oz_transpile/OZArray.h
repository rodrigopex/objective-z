/**
 * @file OZArray.h
 * @brief Immutable array class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits pure-C static array constants.
 */
#pragma once
#import "OZObject.h"
#import "Iterator+Protocol.h"

@interface OZArray<__covariant ObjectType> : OZObject <IteratorProtocol> {
	id *_items;
	unsigned int _count;
	uint16_t _iterIdx;
}

@property (readonly) uint16_t iterIdx;

+ (id)arrayWithObjects:(const id *)objects count:(unsigned int)count;
- (unsigned int)count;
- (id)objectAtIndex:(unsigned int)index;
- (id)objectAtIndexedSubscript:(unsigned int)index;
- (void)enumerateObjectsUsingBlock:(void (^)(id obj, unsigned int idx, BOOL *stop))block;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
- (instancetype)iter;
- (id)next;
@end

@compatibility_alias NSArray OZArray;
