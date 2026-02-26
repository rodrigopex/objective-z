/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZArray.m
 * @brief Immutable array class implementation.
 *
 * Elements are retained on creation and released on dealloc.
 */
#import <objc/OZArray.h>
#import <objc/OZMutableString.h>
#import <objc/objc.h>

#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

@implementation OZArray

+ (id)arrayWithObjects:(const id *)objects count:(unsigned int)count
{
	OZArray *arr = [[OZArray alloc] init];
	if (!arr) {
		return nil;
	}

	if (count > 0) {
		arr->_items = (id *)objc_malloc(sizeof(id) * count);
		if (!arr->_items) {
			[arr release];
			return nil;
		}
		for (unsigned int i = 0; i < count; i++) {
			arr->_items[i] = [objects[i] retain];
		}
	}
	arr->_count = count;

	LOG_DBG("OZArray created count=%u", count);
	return [arr autorelease];
}

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

- (id)objectAtIndexedSubscript:(unsigned int)index
{
	return [self objectAtIndex:index];
}

- (id)description
{
	OZMutableString *desc = [OZMutableString stringWithCString:"("];
	for (unsigned int i = 0; i < _count; i++) {
		if (i > 0) {
			[desc appendCString:", "];
		}
		[desc appendString:[_items[i] description]];
	}
	[desc appendCString:")"];
	return desc;
}

- (void)dealloc
{
	for (unsigned int i = 0; i < _count; i++) {
		[_items[i] release];
	}
	if (_items) {
		objc_free(_items);
	}
	LOG_DBG("OZArray dealloc count=%u", _count);
	[super dealloc];
}

@end
