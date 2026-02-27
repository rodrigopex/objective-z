/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZDictionary.m
 * @brief Immutable dictionary class implementation.
 *
 * Keys and values are retained on creation and released on dealloc.
 * Lookup is linear scan with -isEqual: on keys.
 */
#import <objc/OZDictionary.h>
#import <objc/OZMutableString.h>
#import <objc/objc.h>

#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

@implementation OZDictionary

+ (id)dictionaryWithObjects:(const id *)objects
		    forKeys:(const id *)keys
		      count:(unsigned int)count
{
	OZDictionary *dict = [[OZDictionary alloc] init];
	if (!dict) {
		return nil;
	}

	if (count > 0) {
		/* Single allocation: keys followed by values */
		id *buf = (id *)objc_malloc(sizeof(id) * count * 2);
		if (!buf) {
			[dict release];
			return nil;
		}
		dict->_keys = buf;
		dict->_values = buf + count;
		for (unsigned int i = 0; i < count; i++) {
			dict->_keys[i] = [keys[i] retain];
			dict->_values[i] = [objects[i] retain];
		}
	}
	dict->_count = count;

	LOG_DBG("OZDictionary created count=%u", count);
	return [dict autorelease];
}

- (unsigned int)count
{
	return _count;
}

- (id)objectForKey:(id)key
{
	for (unsigned int i = 0; i < _count; i++) {
		if ([_keys[i] isEqual:key]) {
			return _values[i];
		}
	}
	return nil;
}

- (id)objectForKeyedSubscript:(id)key
{
	return [self objectForKey:key];
}

- (id)description
{
	OZMutableString *desc = [OZMutableString stringWithCString:"{"];
	for (unsigned int i = 0; i < _count; i++) {
		if (i > 0) {
			[desc appendCString:"; "];
		}
		[desc appendString:[_keys[i] description]];
		[desc appendCString:" = "];
		[desc appendString:[_values[i] description]];
	}
	[desc appendCString:"}"];
	return desc;
}

- (unsigned long)countByEnumeratingWithState:(struct NSFastEnumerationState *)state
				     objects:(id *)stackbuf
				       count:(unsigned long)len
{
	(void)stackbuf;
	(void)len;
	if (state->state != 0) {
		return 0;
	}
	state->itemsPtr = _keys;
	state->mutationsPtr = (unsigned long *)self;
	state->state = 1;
	return _count;
}

- (void)dealloc
{
	for (unsigned int i = 0; i < _count; i++) {
		[_keys[i] release];
		[_values[i] release];
	}
	if (_keys) {
		objc_free(_keys); /* single alloc covers both keys and values */
	}
	LOG_DBG("OZDictionary dealloc count=%u", _count);
	[super dealloc];
}

@end
