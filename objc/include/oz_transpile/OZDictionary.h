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

@interface OZDictionary<__covariant KeyType, __covariant ObjectType> : OZObject <IteratorProtocol> {
	id *_keys;
	id *_values;
	unsigned int _count;
	uint16_t _iterIdx;
}

@property (readonly) uint16_t iterIdx;

+ (id)dictionaryWithObjects:(const id *)objects
		    forKeys:(const id *)keys
		      count:(unsigned int)count;
- (unsigned int)count;
- (id)objectForKey:(id)key;
- (id)objectForKeyedSubscript:(id)key;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
- (instancetype)iter;
- (id)next;
@end

@compatibility_alias NSDictionary OZDictionary;
