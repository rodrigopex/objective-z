/**
 * @file OZNumber.h
 * @brief Boxed number class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits pure-C static number constants.
 * Tagged union with 4-byte payload (int32, uint32, float).
 * For double/int64, use OZNumber64 (future).
 */
#pragma once
#import "OZObject.h"

enum oz_number_tag {
	OZ_NUM_INT32 = 0,
	OZ_NUM_UINT32,
	OZ_NUM_FLOAT,
};

union oz_number_value {
	int i32;
	unsigned int u32;
	float f32;
};

@interface OZNumber : OZObject {
	enum oz_number_tag _tag;
	union oz_number_value _value;
}
+ (instancetype)numberWithInt:(int)value;
+ (instancetype)numberWithBool:(BOOL)value;
+ (instancetype)numberWithFloat:(float)value;
+ (instancetype)numberWithUnsignedInt:(unsigned int)value;
- (int)intValue;
- (BOOL)boolValue;
- (float)floatValue;
- (unsigned int)unsignedIntValue;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
- (BOOL)isEqual:(id)anObject;
@end

#ifdef __clang__
@compatibility_alias NSNumber OZNumber;
#endif
