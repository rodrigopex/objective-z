/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Blocks demo — transpiled to pure C via oz_transpile.
 * __block variables become file-scope statics.
 * for-in replaced by index-based for loops.
 */
#import <Foundation/Foundation.h>
#include <zephyr/kernel.h>

/**
 * @interface Sensor
 * @brief Sensor class that stores samples and provides indexed access.
 */
@interface Sensor : OZObject {
	OZArray *_samples;
}
- (id)init;
- (OZArray *)samples;
@end

@implementation Sensor
- (id)init
{
	_samples = @[ @0, @1, @2, @3, @4, @5, @6, @7, @8, @9 ];
	return self;
}
- (OZArray *)samples
{
	return _samples;
}
@end

/*
 * A block at file scope, and a plain C function taking one as a parameter.
 * Both lower to function pointers (#272) -- until then the `^` reached the
 * C compiler verbatim, which no GCC target can parse. Kept here rather than
 * only in the Rust suite because that suite compiles with the host clang,
 * where a surviving `^` is a valid Clang block: the ARM build is the check
 * that means something.
 */
static int (^scale_by_three)(int) = ^(int v) {
  return v * 3;
};

static int apply_twice(int (^op)(int), int v)
{
	return op(op(v));
}

/*
 * A real Zephyr timer wired straight to an inline block -- what replaces
 * OZTimer (#267). `K_TIMER_DEFINE` stores a `k_timer_expiry_t`, and
 * Objective-C refuses block-to-function-pointer conversion in every
 * position, so `OZM` carries it: discarded unparsed on the Objective-C
 * side, expanded to the real macro in the generated C, where the literal
 * has already become a hoisted function's name. See
 * include/oz_sdk/Foundation/OZMacro.h.
 *
 * The block captures nothing -- it reaches its state through a file-scope
 * variable, which the static bar permits and does not count as a capture.
 * That is the constraint every hoisted block lives under, and the reason
 * a callback needing per-instance context uses `k_timer_user_data_get`.
 */
static volatile int timer_fires;

OZM(K_TIMER_DEFINE, demo_timer, ^(struct k_timer *t) {
	(void)t;
	timer_fires = timer_fires + 1;
}, NULL);

#ifdef __OBJC__
/* The definition above is discarded on this side, so Clang needs a
 * declaration for the `k_timer_start` below. Passed through to the
 * generated C, where `__OBJC__` is undefined and the real macro defines
 * it. */
static struct k_timer demo_timer;
#endif

int main(void)
{
	printk("=== Blocks Demo ===\n");

	/* Global block — no captures, immortal */
	int (^global)(void) = ^{
	  return 42;
	};
	printk("Global block: %d\n", global());

	/* __block variable — mutation across block invocations */
	__block int counter = 0;
	void (^increment)(void) = ^{
	  counter++;
	};
	increment();
	increment();
	increment();
	printk("Mutated counter: %d\n", counter);

	/* __block variable — shared state across two blocks */
	__block int nested_val = 77;
	int (^read_val)(void) = ^{
	  return nested_val;
	};
	printk("Nested block: %d\n", read_val());

	/* File-scope block, and one passed to a plain C function */
	printk("File-scope block: %d\n", scale_by_three(5));
	printk("Block through a C function: %d\n", apply_twice(scale_by_three, 2));

	/* Index-based iteration via Sensor */
	Sensor *sensor = [[Sensor alloc] init];

	__block int sum = 0;

	/* for-in lowered to iterator protocol */
	for (OZQ31 *n in [sensor samples]) {
		sum += [n intValue];
	}

	/* Second pass — index-based access */
	OZArray *samples = [sensor samples];
	unsigned int count = [samples count];
	for (unsigned int idx = 0; idx < count; idx++) {
		id sample = [samples objectAtIndex:idx];
		sum += [(OZQ31 *)sample intValue];
	}

	printk("Sensor sum: %d\n", sum);

	/* Dictionary for-in — iterates over keys */
	OZDictionary *dict = @{ @"a" : @10, @"b" : @20, @"c" : @30 };
	__block int dict_sum = 0;
	for (OZString *key in dict) {
		OZQ31 *val = [dict objectForKey:key];
		dict_sum += [val intValue];
	}
	printk("Dict sum: %d\n", dict_sum);

	/* One-shot timer, so the count is deterministic rather than a
	 * function of how long QEMU took. */
	k_timer_start(&demo_timer, K_MSEC(10), K_NO_WAIT);
	k_msleep(100);
	printk("Timer fired: %d\n", timer_fires);

	printk("=== Blocks Demo Complete ===\n");
	return 0;
}
