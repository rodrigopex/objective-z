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
void printk(const char *fmt, ...);

/* ── Sensor class ─────────────────────────────────────────────────── */

@interface Sensor: OZObject {
	int _value;
}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Sensor

/* `@synchronized(self)` here while main already holds `@synchronized(s)` on
 * the same object, so this sample carries the *re-entrant* shape on a
 * single-core board. A k_spinlock cannot be re-locked, so a second acquire
 * would be a defect; the lowering avoids it by recording the owning thread
 * and skipping both lock and unlock when it sees itself (PARITY.md gap W).
 *
 * That guard could only ever be falsified on two cores, where
 * samples/smp_shared's `-bumpNested` drives it -- and `just test-spin-validate`
 * is the reason it is written here too: with CONFIG_SPIN_VALIDATE on, a broken
 * guard fails `z_spin_lock_valid()` on the acquire, so a single-core ARM run
 * catches it as well. Confirmed by disabling the guard and watching this
 * sample fail (#278). Receiver and holder are also spelled differently, `self`
 * against `s`, so no textual check for an identical receiver could tell they
 * alias.
 */
- (void)setValue:(int)v
{
	@synchronized(self) {
		_value = v;
	}
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
