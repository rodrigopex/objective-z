/* Boxed number implementation for OZ transpiler samples. */

#import "OZNumber.h"
#include <stdio.h>

@implementation OZNumber

+ (instancetype)numberWithInt:(int)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_INT32;
	n->_value.i32 = value;
	return n;
}

+ (instancetype)numberWithBool:(BOOL)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_INT32;
	n->_value.i32 = value ? 1 : 0;
	return n;
}

+ (instancetype)numberWithFloat:(float)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_FLOAT;
	n->_value.f32 = value;
	return n;
}

+ (instancetype)numberWithUnsignedInt:(unsigned int)value
{
	OZNumber *n = [[OZNumber alloc] init];
	n->_tag = OZ_NUM_UINT32;
	n->_value.u32 = value;
	return n;
}

- (int)intValue
{
	if (_tag == OZ_NUM_INT32) {
		return _value.i32;
	}
	if (_tag == OZ_NUM_UINT32) {
		return (int)_value.u32;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (int)_value.f32;
	}
	return 0;
}

- (BOOL)boolValue
{
	return [self intValue] != 0;
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
	return 0.0f;
}

- (unsigned int)unsignedIntValue
{
	if (_tag == OZ_NUM_UINT32) {
		return _value.u32;
	}
	if (_tag == OZ_NUM_INT32) {
		return (unsigned int)_value.i32;
	}
	if (_tag == OZ_NUM_FLOAT) {
		return (unsigned int)_value.f32;
	}
	return 0;
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	if (_tag == OZ_NUM_INT32) {
		return snprintf(buf, maxLen, "%d", _value.i32);
	}
	if (_tag == OZ_NUM_UINT32) {
		return snprintf(buf, maxLen, "%u", _value.u32);
	}
	if (_tag == OZ_NUM_FLOAT) {
		return snprintf(buf, maxLen, "%g", (double)_value.f32);
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

- (void)dealloc
{
	/* OZNumber is a compile-time constant and must never be freed. */
}

@end
