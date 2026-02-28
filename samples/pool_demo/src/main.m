/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Static allocation pool demo.
 *
 * Sensor uses a slab pool (OZ_DEFINE_POOL) — zero heap allocation.
 * Gadget has no pool — falls back to the sys_heap allocator.
 */

#import <Foundation/Foundation.h>
#import <objc/objc.h>

/* ── Sensor class ─────────────────────────────────────────────────── */

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

/* ── Gadget class (no pool, uses heap fallback) ───────────────────── */

@interface Gadget : Object {
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
	OZLog("Gadget dealloc (value=%d)", _value);
	[super dealloc];
}

@end

int main(void)
{
	OZLog("=== Static Pool Demo ===");

	/* Allocate 3 Sensors from the slab pool */
	Sensor *s1 = [[Sensor alloc] init];
	[s1 setValue:1];
	OZLog("pool alloc s1 value=%d", [s1 value]);

	Sensor *s2 = [[Sensor alloc] init];
	[s2 setValue:2];
	OZLog("pool alloc s2 value=%d", [s2 value]);

	Sensor *s3 = [[Sensor alloc] init];
	[s3 setValue:3];
	OZLog("pool alloc s3 value=%d", [s3 value]);

	/* Release all — blocks return to slab */
	[s1 release];
	[s2 release];
	[s3 release];

	/* Gadget has no pool — falls back to sys_heap */
	Gadget *g = [[Gadget alloc] init];
	[g setValue:100];
	OZLog("heap alloc g value=%d", [g value]);
	[g release];

	OZLog("=== Demo complete ===");
	return 0;
}
