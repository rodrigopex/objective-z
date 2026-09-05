/* Immutable string implementation for OZ transpiler. */

#import <Foundation/OZString.h>
#include <string.h>

@implementation OZString

- (id)init
{
	self = [super init];
	return self;
}

- (const char *)cString
{
	return _data;
}

- (size_t)length
{
	return _length;
}

- (int)cDescription:(char *)buf maxLength:(size_t)maxLen
{
	/* Both operands unsigned now, so the cast the signed comparison
	 * needed is gone with it. */
	size_t len = (_length < maxLen) ? _length : maxLen;
	memcpy(buf, _data, len);
	return (int)len;
}

- (BOOL)isEqual:(id)anObject
{
	if (self == anObject) {
		return YES;
	}
	OZString *other = (OZString *)anObject;
	if (_length != other->_length) {
		return NO;
	}
	return memcmp(_data, other->_data, _length) == 0;
}

- (BOOL)isEqualToString:(OZString *)aString
{
	if (self == (id)aString) {
		return YES;
	}
	if (aString == nil) {
		return NO;
	}
	if (_length != aString->_length) {
		return NO;
	}
	return memcmp(_data, aString->_data, _length) == 0;
}

- (BOOL)hasPrefix:(OZString *)prefix
{
	if (prefix == nil || prefix->_length > _length) {
		return NO;
	}
	return memcmp(_data, prefix->_data, prefix->_length) == 0;
}

- (BOOL)hasSuffix:(OZString *)suffix
{
	if (suffix == nil || suffix->_length > _length) {
		return NO;
	}
	return memcmp(_data + _length - suffix->_length,
		      suffix->_data, suffix->_length) == 0;
}

- (void)dealloc
{
	/* OZString is a compile-time constant and must never be freed. */
}

@end
