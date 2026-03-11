/* Boxed number implementation for OZ transpiler samples. */

#import <Foundation/OZNumber.h>
#include <stdio.h>

@implementation OZNumber

+ (instancetype)numberWithInt8:(int8_t)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_INT8;
	n->_value.i8 = value;
	return n;
}

+ (instancetype)numberWithUint8:(uint8_t)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_UINT8;
	n->_value.u8 = value;
	return n;
}

+ (instancetype)numberWithInt16:(int16_t)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_INT16;
	n->_value.i16 = value;
	return n;
}

+ (instancetype)numberWithUint16:(uint16_t)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_UINT16;
	n->_value.u16 = value;
	return n;
}

+ (instancetype)numberWithInt32:(int32_t)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_INT32;
	n->_value.i32 = value;
	return n;
}

+ (instancetype)numberWithUint32:(uint32_t)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_UINT32;
	n->_value.u32 = value;
	return n;
}

+ (instancetype)numberWithFloat:(float)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_FLOAT;
	n->_value.f32 = value;
	return n;
}

+ (instancetype)numberWithBool:(BOOL)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_INT8;
	n->_value.i8 = value ? 1 : 0;
	return n;
}

- (int8_t)int8Value
{
	if (_tag == OZ_NUM_INT8) {
		return _value.i8;
	}
	if (_tag == OZ_NUM_UINT8) {
		return (int8_t)_value.u8;
	}
	if (_tag == OZ_NUM_INT16) {
		return (int8_t)_value.i16;
	}
	if (_tag == OZ_NUM_UINT16) {
		return (int8_t)_value.u16;
	}
	if (_tag == OZ_NUM_INT32) {
		return (int8_t)_value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return (int8_t)_value.u32;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (int8_t)_value.f32;
	}
	return 0;
}

- (uint8_t)uint8Value
{
	if (_tag == OZ_NUM_UINT8) {
		return _value.u8;
	}
	if (_tag == OZ_NUM_INT8) {
		return (uint8_t)_value.i8;
	}
	if (_tag == OZ_NUM_INT16) {
		return (uint8_t)_value.i16;
	}
	if (_tag == OZ_NUM_UINT16) {
		return (uint8_t)_value.u16;
	}
	if (_tag == OZ_NUM_INT32) {
		return (uint8_t)_value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return (uint8_t)_value.u32;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (uint8_t)_value.f32;
	}
	return 0;
}

- (int16_t)int16Value
{
	if (_tag == OZ_NUM_INT16) {
		return _value.i16;
	}
	if (_tag == OZ_NUM_UINT16) {
		return (int16_t)_value.u16;
	}
	if (_tag == OZ_NUM_INT8) {
		return (int16_t)_value.i8;
	}
	if (_tag == OZ_NUM_UINT8) {
		return (int16_t)_value.u8;
	}
	if (_tag == OZ_NUM_INT32) {
		return (int16_t)_value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return (int16_t)_value.u32;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (int16_t)_value.f32;
	}
	return 0;
}

- (uint16_t)uint16Value
{
	if (_tag == OZ_NUM_UINT16) {
		return _value.u16;
	}
	if (_tag == OZ_NUM_INT16) {
		return (uint16_t)_value.i16;
	}
	if (_tag == OZ_NUM_INT8) {
		return (uint16_t)_value.i8;
	}
	if (_tag == OZ_NUM_UINT8) {
		return (uint16_t)_value.u8;
	}
	if (_tag == OZ_NUM_INT32) {
		return (uint16_t)_value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return (uint16_t)_value.u32;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (uint16_t)_value.f32;
	}
	return 0;
}

- (int32_t)int32Value
{
	if (_tag == OZ_NUM_INT32) {
		return _value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return (int32_t)_value.u32;
	}
	if (_tag == OZ_NUM_INT8) {
		return (int32_t)_value.i8;
	}
	if (_tag == OZ_NUM_UINT8) {
		return (int32_t)_value.u8;
	}
	if (_tag == OZ_NUM_INT16) {
		return (int32_t)_value.i16;
	}
	if (_tag == OZ_NUM_UINT16) {
		return (int32_t)_value.u16;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (int32_t)_value.f32;
	}
	return 0;
}

- (uint32_t)uint32Value
{
	if (_tag == OZ_NUM_UINT32) {
		return _value.u32;
	}
	if (_tag == OZ_NUM_INT32) {
		return (uint32_t)_value.i32;
	}
	if (_tag == OZ_NUM_INT8) {
		return (uint32_t)_value.i8;
	}
	if (_tag == OZ_NUM_UINT8) {
		return (uint32_t)_value.u8;
	}
	if (_tag == OZ_NUM_INT16) {
		return (uint32_t)_value.i16;
	}
	if (_tag == OZ_NUM_UINT16) {
		return (uint32_t)_value.u16;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (uint32_t)_value.f32;
	}
	return 0;
}

- (float)floatValue
{
	if (_tag == OZ_NUM_FLOAT) {
		return _value.f32;
	}
	if (_tag == OZ_NUM_INT32) {
		return (float)_value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return (float)_value.u32;
	}
	if (_tag == OZ_NUM_INT8) {
		return (float)_value.i8;
	}
	if (_tag == OZ_NUM_UINT8) {
		return (float)_value.u8;
	}
	if (_tag == OZ_NUM_INT16) {
		return (float)_value.i16;
	}
	if (_tag == OZ_NUM_UINT16) {
		return (float)_value.u16;
	}
	return 0.0f;
}

- (BOOL)boolValue
{
	return [self int32Value] != 0;
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	if (_tag == OZ_NUM_INT8) {
		return snprintf(buf, maxLen, "%d", _value.i8);
	}
	if (_tag == OZ_NUM_UINT8) {
		return snprintf(buf, maxLen, "%u", _value.u8);
	}
	if (_tag == OZ_NUM_INT16) {
		return snprintf(buf, maxLen, "%d", _value.i16);
	}
	if (_tag == OZ_NUM_UINT16) {
		return snprintf(buf, maxLen, "%u", _value.u16);
	}
	if (_tag == OZ_NUM_INT32) {
		return snprintf(buf, maxLen, "%d", _value.i32);
	}
	if (_tag == OZ_NUM_UINT32) {
		return snprintf(buf, maxLen, "%u", _value.u32);
	}
	if (_tag == OZ_NUM_FLOAT) {
		return snprintf(buf, maxLen, "%f", (double)_value.f32);
	}
	return 0;
}

- (BOOL)isEqual:(id)anObject
{
	if (self == anObject) {
		return YES;
	}
	OZNumber *other = (OZNumber *)anObject;
	if (_tag != other->_tag) {
		return NO;
	}
	if (_tag == OZ_NUM_INT8) {
		return _value.i8 == other->_value.i8;
	}
	if (_tag == OZ_NUM_UINT8) {
		return _value.u8 == other->_value.u8;
	}
	if (_tag == OZ_NUM_INT16) {
		return _value.i16 == other->_value.i16;
	}
	if (_tag == OZ_NUM_UINT16) {
		return _value.u16 == other->_value.u16;
	}
	if (_tag == OZ_NUM_INT32) {
		return _value.i32 == other->_value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return _value.u32 == other->_value.u32;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return _value.f32 == other->_value.f32;
	}
	return NO;
}

/* Clang literal compatibility: @42 calls numberWithInt: */
+ (instancetype)numberWithInt:(int)value
{
	return [OZNumber numberWithInt32:(int32_t)value];
}

+ (instancetype)numberWithUnsignedInt:(unsigned int)value
{
	return [OZNumber numberWithUint32:(uint32_t)value];
}

- (int)intValue
{
	return (int)[self int32Value];
}

- (unsigned int)unsignedIntValue
{
	return (unsigned int)[self uint32Value];
}

- (void)dealloc
{
	/* OZNumber is a compile-time constant and must never be freed. */
}

@end
