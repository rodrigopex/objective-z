/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * OZHeap — heap manager for OZ objects.
 * Transpiled to plain C — no ObjC runtime needed.
 */

#import <Foundation/OZHeap.h>

@implementation OZHeap

- (id)initWithBuffer:(void *)buf size:(int)size
{
	self = [super init];
	if (self != nil) {
		oz_heap_init(&self->_inner, buf, (size_t)size);
	}
	return self;
}

@end
