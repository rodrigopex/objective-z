/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * MRR (Manual Retain/Release) memory management demo.
 * Demonstrates retain/release/autorelease lifecycle.
 */

#import <objc/objc.h>
#import <objc/OZAutoreleasePool.h>
#include <objc/OZLog.h>

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
	[super dealloc];
}

@end

int main(void)
{
	OZLog("=== MRR Memory Management Demo ===");

	/* Basic retain/release lifecycle */
	Sensor *s = [[Sensor alloc] init];
	[s setValue:42];
	OZLog("retainCount after alloc: %u", [s retainCount]);

	[s retain];
	OZLog("retainCount after retain: %u", [s retainCount]);

	[s release];
	OZLog("retainCount after release: %u", [s retainCount]);

	/* Final release triggers dealloc */
	[s release];

	/* Autorelease pool test */
	OZLog("=== Autorelease pool test ===");
	@autoreleasepool {
		Sensor *a = [[[Sensor alloc] init] autorelease];
		[a setValue:99];
		OZLog("autoreleased sensor value=%d, rc=%u", [a value], [a retainCount]);
		/* a will be released when pool drains */
	}

	OZLog("=== Demo complete ===");
	return 0;
}
