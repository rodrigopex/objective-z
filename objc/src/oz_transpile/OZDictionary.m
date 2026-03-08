/* Immutable dictionary implementation for OZ transpiler samples. */

#import "OZDictionary.h"

@implementation OZDictionary

@synthesize iterIdx = _iterIdx;

- (instancetype)iter
{
	_iterIdx = 0;
	return self;
}

- (id)next
{
	if (_iterIdx >= _count) {
		return nil;
	}
	id ret = _keys[_iterIdx];
	_iterIdx++;
	return ret;
}

- (unsigned int)count
{
	return _count;
}

- (id)objectForKey:(id)key
{
	for (unsigned int i = 0; i < _count; i++) {
		id k = _keys[i];
		if ([k isEqual:key]) {
			return _values[i];
		}
	}
	return nil;
}

- (id)objectForKeyedSubscript:(id)key
{
	return [self objectForKey:key];
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	int pos = 0;
	if (pos < maxLen) {
		buf[pos++] = '{';
	}
	for (unsigned int i = 0; i < _count && pos < maxLen; i++) {
		if (i > 0 && pos + 1 < maxLen) {
			buf[pos++] = ';';
			buf[pos++] = ' ';
		}
		id k = _keys[i];
		pos += [k cDescription:buf + pos maxLength:maxLen - pos];
		if (pos + 2 < maxLen) {
			buf[pos++] = ' ';
			buf[pos++] = '=';
			buf[pos++] = ' ';
		}
		id v = _values[i];
		pos += [v cDescription:buf + pos maxLength:maxLen - pos];
	}
	if (pos < maxLen) {
		buf[pos++] = '}';
	}
	return pos;
}

- (void)dealloc
{
	/* OZDictionary is a compile-time constant and must never be freed. */
}

@end
