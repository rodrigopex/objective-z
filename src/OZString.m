/* Constant string implementation for OZ transpiler samples. */

#import <Foundation/OZString.h>
#include <string.h>

@implementation OZString

- (const char *)cStr
{
	return _data;
}

- (unsigned int)length
{
	return _length;
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	int len = (_length < (unsigned int)maxLen) ? (int)_length : maxLen;
	memcpy(buf, _data, len);
	return len;
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

- (void)dealloc
{
	/* OZString is a compile-time constant and must never be freed. */
}

@end
