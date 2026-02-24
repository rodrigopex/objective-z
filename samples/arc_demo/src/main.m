/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * ARC (Automatic Reference Counting) demo.
 * Compiled with -fobjc-arc — no manual retain/release needed.
 */

#import <objc/OZObject.h>
#import <objc/OZAutoreleasePool.h>
#include <zephyr/kernel.h>

@interface Sensor : OZObject {
	int _value;
}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Sensor

- (id)init
{
	self = [super init];
	return self;
}

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
	printk("Sensor dealloc (value=%d)\n", _value);
	/* ARC calls [super dealloc] automatically */
}

@end

static Sensor *createSensor(int v)
{
	Sensor *s = [[Sensor alloc] init];
	[s setValue:v];
	return s;
}

int main(void)
{
	printk("=== ARC Memory Management Demo ===\n");

	/* Scope test: ARC releases s when it goes out of scope */
	{
		Sensor *s = createSensor(42);
		printk("Sensor created, value=%d\n", [s value]);
	}
	/* s is released here by ARC → dealloc fires */

	/* @autoreleasepool test */
	printk("@autoreleasepool test\n");
	@autoreleasepool {
		Sensor *a = createSensor(99);
		printk("pool sensor value=%d\n", [a value]);
	}
	/* pool drains, a is released → dealloc fires */

	printk("=== Demo complete ===\n");
	return 0;
}
