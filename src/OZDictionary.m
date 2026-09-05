/* Immutable dictionary implementation for OZ transpiler samples. */

#import <Foundation/OZDictionary.h>

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

- (size_t)count
{
	return _count;
}

- (id)objectForKey:(id)key
{
	for (size_t i = 0; i < _count; i++) {
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

- (int)cDescription:(char *)buf maxLength:(size_t)maxLen
{
	/* Unsigned alongside `maxLen`. `pos` can reach `maxLen` but never
	 * pass it -- every multi-byte write is guarded by `pos + n < maxLen`
	 * -- so the `maxLen - pos` below is zero at worst, never wrapped. */
	size_t pos = 0;
	if (pos < maxLen) {
		buf[pos++] = '{';
	}
	for (size_t i = 0; i < _count && pos < maxLen; i++) {
		if (i > 0 && pos + 1 < maxLen) {
			buf[pos++] = ';';
			buf[pos++] = ' ';
		}
		id k = _keys[i];
		int written_k = [k cDescription:buf + pos maxLength:maxLen - pos];
		if (written_k > 0) {
			pos += (size_t)written_k;
		}
		if (pos + 2 < maxLen) {
			buf[pos++] = ' ';
			buf[pos++] = '=';
			buf[pos++] = ' ';
		}
		id v = _values[i];
		int written_v = [v cDescription:buf + pos maxLength:maxLen - pos];
		if (written_v > 0) {
			pos += (size_t)written_v;
		}
	}
	if (pos < maxLen) {
		buf[pos++] = '}';
	}
	return (int)pos;
}

@end
