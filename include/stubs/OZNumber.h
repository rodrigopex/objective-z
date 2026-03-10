/**
 * @file OZNumber.h
 * @brief Boxed number class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits pure-C static number constants.
 * Tagged union with fixed-width integer types and float.
 * For double/int64, use OZNumber64 (future).
 */
#pragma once
#import "OZObject.h"

enum oz_number_tag {
	OZ_NUM_INT8 = 0,
	OZ_NUM_UINT8,
	OZ_NUM_INT16,
	OZ_NUM_UINT16,
	OZ_NUM_INT32,
	OZ_NUM_UINT32,
	OZ_NUM_FLOAT,
};

union oz_number_value {
	int8_t i8;
	uint8_t u8;
	int16_t i16;
	uint16_t u16;
	int32_t i32;
	uint32_t u32;
	float f32;
};

@interface OZNumber : OZObject {
	enum oz_number_tag _tag;
	union oz_number_value _value;
}
+ (instancetype)numberWithInt8:(int8_t)value;
+ (instancetype)numberWithUint8:(uint8_t)value;
+ (instancetype)numberWithInt16:(int16_t)value;
+ (instancetype)numberWithUint16:(uint16_t)value;
+ (instancetype)numberWithInt32:(int32_t)value;
+ (instancetype)numberWithUint32:(uint32_t)value;
+ (instancetype)numberWithFloat:(float)value;
+ (instancetype)numberWithBool:(BOOL)value;
/* Clang emits calls to these for @42 / @42U literals */
+ (instancetype)numberWithInt:(int)value;
+ (instancetype)numberWithUnsignedInt:(unsigned int)value;
- (int8_t)int8Value;
- (uint8_t)uint8Value;
- (int16_t)int16Value;
- (uint16_t)uint16Value;
- (int32_t)int32Value;
- (uint32_t)uint32Value;
/* Convenience aliases for Clang literal compatibility */
- (int)intValue;
- (unsigned int)unsignedIntValue;
- (float)floatValue;
- (BOOL)boolValue;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
- (BOOL)isEqual:(id)anObject;
@end

#ifdef __clang__
@compatibility_alias NSNumber OZNumber;
#endif
