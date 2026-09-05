/* Immutable array implementation for OZ transpiler samples. */

#import <Foundation/OZArray.h>

@implementation OZArray

@synthesize iterIdx = _iterIdx;

- (size_t)count
{
	return _count;
}

- (id)objectAtIndex:(size_t)index
{
	if (index >= _count) {
		return nil;
	}
	return _items[index];
}

- (int)cDescription:(char *)buf maxLength:(size_t)maxLen
{
	/* `pos` is unsigned alongside `maxLen`, which is safe because every
	 * subtraction below is already guarded by `pos < maxLen` -- the loop
	 * condition and the `if`s exist for exactly that. Left signed it would
	 * mix signedness in every comparison instead. */
	size_t pos = 0;
	if (pos < maxLen) {
		buf[pos++] = '(';
	}
	for (size_t i = 0; i < _count && pos < maxLen; i++) {
		if (i > 0 && pos + 1 < maxLen) {
			buf[pos++] = ',';
			buf[pos++] = ' ';
		}
		id elem = _items[i];
		/* `pos < maxLen` holds here, from the loop condition, so the
		 * remaining capacity cannot underflow. The result is `int`, and
		 * a negative one would wrap `pos` past `maxLen` if added
		 * blindly -- which the old all-signed version did, then indexed
		 * with it. */
		int written = [elem cDescription:buf + pos maxLength:maxLen - pos];
		if (written > 0) {
			pos += (size_t)written;
		}
	}
	if (pos < maxLen) {
		buf[pos++] = ')';
	}
	return (int)pos;
}

- (id)objectAtIndexedSubscript:(size_t)index
{
	return [self objectAtIndex:index];
}

- (void)enumerateObjectsUsingBlock:(void (^)(id obj, size_t idx, BOOL *stop))block
{
	BOOL stop = NO;
	for (size_t i = 0; i < _count && !stop; i++) {
		block(_items[i], i, &stop);
	}
}

- (instancetype)iter {
	_iterIdx = 0;
	return self;
}
- (id)next {
	if (_iterIdx >= _count) {
		return nil;
	}

	id ret = _items[_iterIdx];

	_iterIdx++;

	return ret;
}

@end
