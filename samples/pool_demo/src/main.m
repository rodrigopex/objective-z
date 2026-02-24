/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Static allocation pool demo.
 *
 * Sensor uses a slab pool (OZ_DEFINE_POOL) — zero heap allocation.
 * Gadget has no pool — falls back to the sys_heap allocator.
 */

#import <objc/OZObject.h>
#import <objc/OZAutoreleasePool.h>
#include <zephyr/kernel.h>

/* ── Sensor class ─────────────────────────────────────────────────── */

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

/* ── Gadget class (no pool, uses heap fallback) ───────────────────── */

@interface Gadget : OZObject {
	int _value;
}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Gadget

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
	printk("Gadget dealloc (value=%d)\n", _value);
	[super dealloc];
}

@end

int main(void)
{
	printk("=== Static Pool Demo ===\n");

	/* Allocate 3 Sensors from the slab pool */
	Sensor *s1 = [[Sensor alloc] init];
	[s1 setValue:1];
	printk("pool alloc s1 value=%d\n", [s1 value]);

	Sensor *s2 = [[Sensor alloc] init];
	[s2 setValue:2];
	printk("pool alloc s2 value=%d\n", [s2 value]);

	Sensor *s3 = [[Sensor alloc] init];
	[s3 setValue:3];
	printk("pool alloc s3 value=%d\n", [s3 value]);

	/* Release all — blocks return to slab */
	[s1 release];
	[s2 release];
	[s3 release];

	/* Gadget has no pool — falls back to sys_heap */
	Gadget *g = [[Gadget alloc] init];
	[g setValue:100];
	printk("heap alloc g value=%d\n", [g value]);
	[g release];

	printk("=== Demo complete ===\n");
	return 0;
}
