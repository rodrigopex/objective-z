/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * ARC (Automatic Reference Counting) demo.
 * Compiled with -fobjc-arc — no manual retain/release needed.
 */

#import <objc/OZObject.h>
#import <objc/OZAutoreleasePool.h>
#include <zephyr/kernel.h>

@interface Sensor: OZObject {
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

@interface Driver: OZObject {
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
	printk("Driver created (sensor value=%d)\n", [_sensor value]);
	return self;
}
- (void)dealloc
{
	printk("Driver dealloc (sensor value=%d)\n", [_sensor value]);
}
@end

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

	printk("=== Demo main complete ===\n");
	return 0;
}

static void arc_demo_extra_thread_entry(void *p1, void *p2, void *p3)
{
	ARG_UNUSED(p1);
	ARG_UNUSED(p2);
	ARG_UNUSED(p3);
	printk("=== Demo Extra thread started ===\n");
	Driver *d = [[Driver alloc] init:250];
	d.sensor.value = 100;
	printk("=== Demo Extra thread started ===\n");
}

K_THREAD_DEFINE(arc_demo_thread, 1024, arc_demo_extra_thread_entry, NULL, NULL, NULL, 7, 0, 0);
