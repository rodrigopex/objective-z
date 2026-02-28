/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * ARC memory management demo.
 * Demonstrates scope-based lifetime and autorelease pools.
 */

#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <objc/arc.h>
#include <objc/runtime.h>

@interface Sensor : Object {
	int _value;
}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Sensor

- (void)setValue:(int)v
{
	_value = v;
}

- (int)value
{
	return _value;
}

- (void)dealloc
{
	OZLog("Sensor dealloc (value=%d)", _value);
}

@end

int main(void)
{
	OZLog("=== ARC Memory Management Demo ===");

	/* ARC scope-based lifecycle */
	{
		Sensor *s = [[Sensor alloc] init];
		[s setValue:42];
		OZLog("rc after alloc: %u", __objc_refcount_get(s));
		/* s released automatically at scope exit */
	}

	/* Autorelease pool test */
	OZLog("=== Autorelease pool test ===");
	@autoreleasepool {
		Sensor *a = [[Sensor alloc] init];
		[a setValue:99];
		OZLog("sensor value=%d, rc=%u", [a value], __objc_refcount_get(a));
		/* a released when pool drains */
	}

	OZLog("=== Demo complete ===");
	return 0;
}
