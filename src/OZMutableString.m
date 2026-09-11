/* Mutable string implementation for OZ transpiler. */

#import <Foundation/OZMutableString.h>
/*
 * `<stddef.h>` for `NULL`, named here rather than taken from the
 * `OZString.h` -> `OZObject.h` chain that also supplies it. This file used
 * to carry `#define NULL ((void *)0)` of its own instead, added alongside
 * the libc stubs for the Clang AST dump (d7ec624) -- but no dump path has
 * ever needed it: `tests/behavior/include/stubs/` stubs `stdlib.h`,
 * `string.h` and `stdio.h` and deliberately not `stddef.h`, and every
 * dump (`cmake/ObjcClang.cmake`, `tests/tools/compile_and_run.py`,
 * `tests/smoke/run.py`, the Rust harness, `just ast-dump`) reaches the
 * real one. It was `#ifndef`-guarded, so it never expanded either.
 *
 * It had to go regardless of being dead: file-scope text ahead of an
 * `@implementation` is spliced into the *generated header* for this
 * origin, which every Foundation translation unit includes -- see the
 * comment on `_oz_write_default_description` in `src/OZObject.m`. So a
 * `.m` redefining a standard library macro redefines it for the whole
 * Foundation, and this one did (#422).
 */
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

@implementation OZMutableString

- (id)initWithCString:(const char *)str
{
	self = [super init];
	/* `+alloc` zeroed `_data`, and `-init` may run more than once on the
	 * same object, so this is a no-op on the first run and frees the
	 * previous buffer on any later one. Without it a second
	 * initialisation leaks the first buffer, which `-dealloc` can never
	 * make up for -- by then `_data` names only the last one (#405).
	 */
	free((void *)_data);
	_data = NULL;
	_length = 0;
	_capacity = 0;
	if (str == NULL) {
		size_t cap = 16;
		char *buf = (char *)malloc(cap);
		if (buf == NULL) {
			return nil;
		}
		buf[0] = '\0';
		_data = buf;
		_length = 0;
		_capacity = cap;
	} else {
		size_t len = strlen(str);
		size_t cap = len < 16 ? 16 : len * 2;
		char *buf = (char *)malloc(cap);
		if (buf == NULL) {
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
		return [self initWithCString:NULL];
	}
	return [self initWithCString:[aString cString]];
}

- (id)initWithCapacity:(size_t)capacity
{
	self = [super init];
	/* `+alloc` zeroed `_data`, and `-init` may run more than once on the
	 * same object, so this is a no-op on the first run and frees the
	 * previous buffer on any later one. Without it a second
	 * initialisation leaks the first buffer, which `-dealloc` can never
	 * make up for -- by then `_data` names only the last one (#405).
	 */
	free((void *)_data);
	_data = NULL;
	_length = 0;
	_capacity = 0;
	size_t cap = capacity < 16 ? 16 : capacity;
	char *buf = (char *)malloc(cap);
	if (buf == NULL) {
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
	if (str == NULL) {
		return;
	}
	size_t addLen = strlen(str);
	if (addLen == 0) {
		return;
	}
	size_t newLen = _length + addLen;
	if (newLen + 1 > _capacity) {
		size_t newCap = _capacity;
		while (newCap < newLen + 1) {
			newCap = newCap * 2;
		}
		char *newBuf = (char *)malloc(newCap);
		if (newBuf == NULL) {
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
	size_t len = [aString length];
	if (len + 1 > _capacity) {
		size_t newCap = _capacity;
		while (newCap < len + 1) {
			newCap = newCap * 2;
		}
		char *newBuf = (char *)malloc(newCap);
		if (newBuf == NULL) {
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
