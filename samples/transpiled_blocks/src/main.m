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

	/* Index-based iteration via Sensor */
	Sensor *sensor = [[Sensor alloc] init];

	__block int sum = 0;

	/* for-in lowered to iterator protocol */
	for (OZNumber *n in [sensor samples]) {
		sum += [n intValue];
	}

	/* Second pass — index-based access */
	OZArray *samples = [sensor samples];
	unsigned int count = [samples count];
	for (unsigned int idx = 0; idx < count; idx++) {
		id sample = [samples objectAtIndex:idx];
		sum += [(OZNumber *)sample intValue];
	}

	printk("Sensor sum: %d\n", sum);

	printk("=== Blocks Demo Complete ===\n");
	return 0;
}
