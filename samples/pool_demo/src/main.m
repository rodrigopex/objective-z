/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Transpiled pool demo.
 *
 * Sensor uses a slab pool — zero heap allocation.
 * Each @autoreleasepool iteration releases the sensor,
 * returning the slab block for the next iteration.
 */

#import <Foundation/OZObject.h>

/* printk declared here so Clang AST dump works without Zephyr generated
 * headers.  The transpiler emits the real #include <zephyr/sys/printk.h>
 * in the generated C output. */
int printk(const char *fmt, ...);

/* ── Sensor class ─────────────────────────────────────────────────── */

@interface Sensor: OZObject {
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
	printk("=== Static Pool Demo ===\n");

	/* Allocate 3 Sensors in a loop with @autoreleasepool.
	 * Each iteration's pool scope releases the sensor,
	 * returning the slab block for the next iteration.
	 */
	for (int i = 1; i <= 3; i++) {
		@autoreleasepool {
			Sensor *s = [[Sensor alloc] init];
			@synchronized(s) {
				[s setValue:i];
			}
			printk("pool alloc sensor value=%d\n", [s value]);
			/* ARC releases s at @autoreleasepool scope exit */
		}
	}

	printk("=== Demo complete ===\n");
	return 0;
}
