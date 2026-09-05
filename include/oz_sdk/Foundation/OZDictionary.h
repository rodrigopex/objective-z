/**
 * @file OZDictionary.h
 * @brief Immutable dictionary class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits pure-C static dictionary constants.
 */
#pragma once
#import "OZObject.h"
#import "Iterator+Protocol.h"

struct NSFastEnumerationState;

@interface OZDictionary<__covariant KeyType, __covariant ObjectType> : OZObject <IteratorProtocol> {
	__unsafe_unretained id *_keys;
	__unsafe_unretained id *_values;
	size_t _count;
	uint16_t _iterIdx;
}

@property (readonly) uint16_t iterIdx;

+ (id)dictionaryWithObjects:(const id *)objects
		    forKeys:(const id *)keys
		      count:(size_t)count;
- (size_t)count;
- (id)objectForKey:(id)key;
- (id)objectForKeyedSubscript:(id)key;
- (unsigned long)countByEnumeratingWithState:(struct NSFastEnumerationState *)state
				     objects:(__unsafe_unretained id *)stackbuf
				       count:(unsigned long)len;
- (int)cDescription:(char *)buf maxLength:(size_t)maxLen;
- (instancetype)iter;
- (id)next;
@end

@compatibility_alias NSDictionary OZDictionary;
