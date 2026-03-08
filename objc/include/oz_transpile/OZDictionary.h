/**
 * @file OZDictionary.h
 * @brief Immutable dictionary class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits pure-C static dictionary constants.
 */
#pragma once
#import "OZObject.h"

@interface OZDictionary : OZObject {
	id *_keys;
	id *_values;
	unsigned int _count;
}
+ (id)dictionaryWithObjects:(const id *)objects
		    forKeys:(const id *)keys
		      count:(unsigned int)count;
- (unsigned int)count;
- (id)objectForKey:(id)key;
- (id)objectForKeyedSubscript:(id)key;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end

@compatibility_alias NSDictionary OZDictionary;
