/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * MRR (Manual Retain/Release) memory management demo.
 * Demonstrates OZObject retain/release/autorelease lifecycle.
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
	[super dealloc];
}

@end

int main(void)
{
	printk("=== MRR Memory Management Demo ===\n");

	/* Basic retain/release lifecycle */
	Sensor *s = [[Sensor alloc] init];
	[s setValue:42];
	printk("retainCount after alloc: %u\n", [s retainCount]);

	[s retain];
	printk("retainCount after retain: %u\n", [s retainCount]);

	[s release];
	printk("retainCount after release: %u\n", [s retainCount]);

	/* Final release triggers dealloc */
	[s release];

	/* Autorelease pool test */
	printk("=== Autorelease pool test ===\n");
	@autoreleasepool {
		Sensor *a = [[[Sensor alloc] init] autorelease];
		[a setValue:99];
		printk("autoreleased sensor value=%d, rc=%u\n", [a value], [a retainCount]);
		/* a will be released when pool drains */
	}

	printk("=== Demo complete ===\n");
	return 0;
}
