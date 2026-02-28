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
#import <Foundation/Foundation.h>
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
@property (nonatomic, readonly) OZArray * _Nonnull samples;
- (id)init;
- (void)iterateSamplesUsingBlock:(void (^)(id obj, unsigned int idx, BOOL *stop))callback;
@end
/**
 * @implementation Sensor
 */
@implementation Sensor
@synthesize samples = _samples;
- (id)init
{
	_samples = @[ @0, @1, @2, @3, @4, @5, @6, @7, @8, @9 ];
	return self;
}

- (void)iterateSamplesUsingBlock:(void (^)(id obj, unsigned int idx, BOOL *stop))callback
{
	[_samples enumerateObjectsUsingBlock:callback];
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
	printk("Capturing block: %d\n", capturing());

	/* __block variable — mutation across block invocations */
	__block int counter = 0;
	VoidBlock increment = ^{
	  counter++;
	};
	increment();
	increment();
	increment();
	printk("Mutated counter: %d\n", counter);

	/* Nested block — outer captures inner */
	int nested_val = 77;
	IntBlock inner = ^{
	  return nested_val;
	};
	IntBlock outer = ^{
	  return inner();
	};
	printk("Nested block: %d\n", outer());

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

	printk("=== Blocks Demo Complete ===\n");
	return 0;
}
