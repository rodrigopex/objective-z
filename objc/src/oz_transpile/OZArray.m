/* Immutable array implementation for OZ transpiler samples. */

#import "OZArray.h"

@implementation OZArray

- (unsigned int)count
{
	return _count;
}

- (id)objectAtIndex:(unsigned int)index
{
	if (index >= _count) {
		return nil;
	}
	return _items[index];
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	int pos = 0;
	if (pos < maxLen) {
		buf[pos++] = '(';
	}
	for (unsigned int i = 0; i < _count && pos < maxLen; i++) {
		if (i > 0 && pos + 1 < maxLen) {
			buf[pos++] = ',';
			buf[pos++] = ' ';
		}
		id elem = _items[i];
		pos += [elem cDescription:buf + pos maxLength:maxLen - pos];
	}
	if (pos < maxLen) {
		buf[pos++] = ')';
	}
	return pos;
}

- (id)objectAtIndexedSubscript:(unsigned int)index
{
	return [self objectAtIndex:index];
}

- (void)enumerateObjectsUsingBlock:(void (^)(id obj, unsigned int idx, BOOL *stop))block
{
	BOOL stop = NO;
	for (unsigned int i = 0; i < _count && !stop; i++) {
		block(_items[i], i, &stop);
	}
}

- (void)dealloc
{
	/* OZArray is a compile-time constant and must never be freed. */
}

@end
