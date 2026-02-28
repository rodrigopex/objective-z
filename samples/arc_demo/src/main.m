/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * ARC (Automatic Reference Counting) demo.
 * Compiled with -fobjc-arc — no manual retain/release needed.
 */

#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <zephyr/kernel.h>

@interface Sensor: Object {
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
	OZLog("Sensor dealloc (value=%d)", _value);
	/* ARC calls [super dealloc] automatically */
}

@end

static Sensor *createSensor(int v)
{
	Sensor *s = [[Sensor alloc] init];
	[s setValue:v];
	return s;
}

@interface Driver: Object {
	Sensor *_sensor;
}
@property(nonatomic, strong) Sensor *sensor;
@end

@implementation Driver
@synthesize sensor = _sensor;
- (id)init:(int)newValue
{
	self = [super init];
	_sensor = createSensor(newValue);
	OZLog("Driver created (sensor value=%d)", [_sensor value]);
	return self;
}
- (void)dealloc
{
	OZLog("Driver dealloc (sensor value=%d)", [_sensor value]);
}
@end

int main(void)
{
	OZLog("=== ARC Memory Management Demo ===");

	/* Scope test: ARC releases s when it goes out of scope */
	{
		Sensor *s = createSensor(42);
		OZLog("Sensor created, value=%d", [s value]);
	}
	/* s is released here by ARC → dealloc fires */

	/* @autoreleasepool test */
	OZLog("@autoreleasepool test");
	@autoreleasepool {
		Sensor *a = createSensor(99);
		OZLog("pool sensor value=%d", [a value]);
	}
	/* pool drains, a is released → dealloc fires */

	OZLog("=== Demo main complete ===");
	return 0;
}

static void arc_demo_extra_thread_entry(void *p1, void *p2, void *p3)
{
	ARG_UNUSED(p1);
	ARG_UNUSED(p2);
	ARG_UNUSED(p3);
	OZLog("=== Demo Extra thread started ===");
	Driver *d = [[Driver alloc] init:250];
	d.sensor.value = 100;
	OZLog("=== Demo Extra thread started ===");
}

K_THREAD_DEFINE(arc_demo_thread, 1024, arc_demo_extra_thread_entry, NULL, NULL, NULL, 7, 0, 0);
