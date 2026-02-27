/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.m
 * @brief Blocks (closures) demo for Objective-Z.
 *
 * Demonstrates global blocks, capturing blocks, __block variables,
 * and nested blocks on Zephyr RTOS.
 */
#import <objc/objc.h>
#import <objc/blocks.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

typedef int (^IntBlock)(void);
typedef void (^VoidBlock)(void);

/**
 * @interface Sensor
 * @brief Sensor class that stores samples and reads them via a block callback.
 */
@interface Sensor : Object {
	OZArray *_samples;
}
@property (readonly) OZArray *samples;
- (id)init;
- (void)iterateSamplesUsingBlock:(void (^)(id obj, unsigned int idx, BOOL *stop))callback;
- (void)dealloc;
@end
/**
 * @implementation Sensor
 */
@implementation Sensor
@synthesize samples = _samples;
- (id)init
{
	_samples = [@[ @0, @1, @2, @3, @4, @5, @6, @7, @8, @9 ] retain];
	return self;
}

- (void)iterateSamplesUsingBlock:(void (^)(id obj, unsigned int idx, BOOL *stop))callback
{
	[_samples enumerateObjectsUsingBlock:callback];
}
- (void)dealloc
{
	[_samples release];
	[super dealloc];
}

@end

int main(void)
{
	printk("=== Blocks Demo ===\n");

	/* Global block — no captures, immortal */
	IntBlock global = ^{
	  return 42;
	};
	printk("Global block: %d\n", global());

	/* Capturing block — captures local variable */
	int value = 99;
	IntBlock capturing = ^{
	  return value;
	};
	void *copied = _Block_copy(capturing);
	printk("Capturing block: %d\n", ((IntBlock)copied)());
	_Block_release(copied);

	/* __block variable — mutation across block invocations */
	__block int counter = 0;
	VoidBlock increment = ^{
	  counter++;
	};
	void *inc_copy = _Block_copy(increment);
	((VoidBlock)inc_copy)();
	((VoidBlock)inc_copy)();
	((VoidBlock)inc_copy)();
	printk("Mutated counter: %d\n", counter);
	_Block_release(inc_copy);

	/* Nested block — outer captures inner */
	int nested_val = 77;
	IntBlock inner = ^{
	  return nested_val;
	};
	IntBlock outer = ^{
	  return inner();
	};
	void *outer_copy = _Block_copy(outer);
	printk("Nested block: %d\n", ((IntBlock)outer_copy)());
	_Block_release(outer_copy);

	/* enumerateObjectsUsingBlock: via Sensor */
	Sensor *sensor = [[Sensor alloc] init];

	__block int sum = 0;

	[sensor iterateSamplesUsingBlock:^(id obj, unsigned int idx, BOOL *stop) {
	  OZLog("Sensor sample [%d]: %@", idx, obj);
	  sum += [obj intValue];
	  *stop = NO;
	}];

	for (id sample in sensor.samples) {
		sum += [sample intValue];
	}

	printk("Sensor sum: %d\n", sum);
	[sensor release];

	printk("=== Blocks Demo Complete ===\n");
	return 0;
}
