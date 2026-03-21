/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * OZHeap — heap manager for OZ objects.
 * Transpiled to plain C — no ObjC runtime needed.
 */

#import <Foundation/OZHeap.h>

@implementation OZHeap

+ (void)initHeap:(OZHeap *)heap buffer:(void *)buf size:(int)size
{
	oz_heap_init(&heap->_inner, buf, (size_t)size);
}

@end
