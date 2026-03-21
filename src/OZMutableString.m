/* Mutable string implementation for OZ transpiler. */

#import <Foundation/OZMutableString.h>
#include <stdlib.h>
#include <string.h>

@implementation OZMutableString

- (id)initWithCString:(const char *)str
{
	self = [super init];
	if (str == nil) {
		unsigned int cap = 16;
		char *buf = (char *)malloc(cap);
		if (buf == nil) {
			return nil;
		}
		buf[0] = '\0';
		_data = buf;
		_length = 0;
		_capacity = cap;
	} else {
		unsigned int len = (unsigned int)strlen(str);
		unsigned int cap = len < 16 ? 16 : len * 2;
		char *buf = (char *)malloc(cap);
		if (buf == nil) {
			return nil;
		}
		memcpy(buf, str, len + 1);
		_data = buf;
		_length = len;
		_capacity = cap;
	}
	return self;
}

- (id)initWithString:(OZString *)aString
{
	if (aString == nil) {
		return [self initWithCString:nil];
	}
	return [self initWithCString:[aString cString]];
}

- (id)initWithCapacity:(unsigned int)capacity
{
	self = [super init];
	unsigned int cap = capacity < 16 ? 16 : capacity;
	char *buf = (char *)malloc(cap);
	if (buf == nil) {
		return nil;
	}
	buf[0] = '\0';
	_data = buf;
	_length = 0;
	_capacity = cap;
	return self;
}

- (void)appendCString:(const char *)str
{
	if (str == nil) {
		return;
	}
	unsigned int addLen = (unsigned int)strlen(str);
	if (addLen == 0) {
		return;
	}
	unsigned int newLen = _length + addLen;
	if (newLen + 1 > _capacity) {
		unsigned int newCap = _capacity;
		while (newCap < newLen + 1) {
			newCap = newCap * 2;
		}
		char *newBuf = (char *)malloc(newCap);
		if (newBuf == nil) {
			return;
		}
		memcpy(newBuf, _data, _length);
		free((void *)_data);
		_data = newBuf;
		_capacity = newCap;
	}
	memcpy((char *)_data + _length, str, addLen + 1);
	_length = newLen;
}

- (void)appendString:(OZString *)aString
{
	if (aString == nil) {
		return;
	}
	[self appendCString:[aString cString]];
}

- (void)setString:(OZString *)aString
{
	if (aString == nil) {
		((char *)_data)[0] = '\0';
		_length = 0;
		return;
	}
	const char *src = [aString cString];
	unsigned int len = [aString length];
	if (len + 1 > _capacity) {
		unsigned int newCap = _capacity;
		while (newCap < len + 1) {
			newCap = newCap * 2;
		}
		char *newBuf = (char *)malloc(newCap);
		if (newBuf == nil) {
			return;
		}
		free((void *)_data);
		_data = newBuf;
		_capacity = newCap;
	}
	memcpy((void *)_data, src, len + 1);
	_length = len;
}

- (void)dealloc
{
	free((void *)_data);
}

@end
