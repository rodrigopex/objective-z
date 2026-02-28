/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZMutableString.m
 * @brief Mutable string class implementation with heap-allocated buffer.
 */
#import <Foundation/OZMutableString.h>
#import <objc/objc.h>
#include <string.h>

@implementation OZMutableString

+ (id)stringWithCString:(const char *)str
{
	OZMutableString *s = [[OZMutableString alloc] init];
	if (!s) {
		return nil;
	}
	if (str) {
		unsigned int len = (unsigned int)strlen(str);
		unsigned int cap = 64;
		while (cap < len + 1) {
			cap *= 2;
		}
		s->_buf = (char *)objc_malloc(cap);
		if (!s->_buf) {
			[s release];
			return nil;
		}
		memcpy(s->_buf, str, len + 1);
		s->_length = len;
		s->_capacity = cap;
	}
	return [s autorelease];
}

- (void)appendCString:(const char *)str
{
	if (!str) {
		return;
	}
	unsigned int appendLen = (unsigned int)strlen(str);
	if (appendLen == 0) {
		return;
	}

	unsigned int needed = _length + appendLen + 1;
	if (needed > _capacity) {
		unsigned int newCap = (_capacity > 0) ? _capacity : 64;
		while (newCap < needed) {
			newCap *= 2;
		}
		char *newBuf = (char *)objc_realloc(_buf, newCap);
		if (!newBuf) {
			return;
		}
		_buf = newBuf;
		_capacity = newCap;
	}

	memcpy(_buf + _length, str, appendLen + 1);
	_length += appendLen;
}

- (void)appendString:(id<OZStringProtocol>)str
{
	if (!str) {
		return;
	}
	[self appendCString:[str cStr]];
}

- (const char *)cStr
{
	if (!_buf) {
		return "";
	}
	return _buf;
}

- (unsigned int)length
{
	return _length;
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	if (!_buf || _length == 0) {
		return 0;
	}
	int len = (_length < (unsigned int)maxLen) ? (int)_length : maxLen;
	memcpy(buf, _buf, len);
	return len;
}

- (void)dealloc
{
	if (_buf) {
		objc_free(_buf);
	}
	[super dealloc];
}

@end
