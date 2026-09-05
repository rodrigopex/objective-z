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

struct NSFastEnumerationState;

@interface OZArray<__covariant ObjectType> : OZObject <IteratorProtocol> {
	__unsafe_unretained id *_items;
	size_t _count;
	uint16_t _iterIdx;
}

@property (readonly) uint16_t iterIdx;

+ (id)arrayWithObjects:(const id *)objects count:(size_t)count;
- (size_t)count;
- (id)objectAtIndex:(size_t)index;
- (id)objectAtIndexedSubscript:(size_t)index;
- (void)enumerateObjectsUsingBlock:(void (^)(id obj, size_t idx, BOOL *stop))block;
- (unsigned long)countByEnumeratingWithState:(struct NSFastEnumerationState *)state
				     objects:(__unsafe_unretained id *)stackbuf
				       count:(unsigned long)len;
- (int)cDescription:(char *)buf maxLength:(size_t)maxLen;
- (instancetype)iter;
- (id)next;
@end

@compatibility_alias NSArray OZArray;
