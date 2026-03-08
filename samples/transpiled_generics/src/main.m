/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.m
 * @brief Transpiled generics demo for Objective-Z.
 *
 * Demonstrates typed collections, number literals, and non-capturing
 * blocks transpiled to pure C via oz_transpile.
 */
#import <Foundation/Foundation.h>
#include <zephyr/kernel.h>

int main(void)
{
	printk("=== Transpiled Generics Demo ===\n");

	@autoreleasepool {
		/* Array of numbers */
		OZArray *numbers = @[ @10, @20, @30 ];
		OZNumber *first = [numbers objectAtIndex:0];
		OZLog("numbers[0] = %@", first);
		OZLog("numbers count = %d", [numbers count]);

		/* Array of strings — manual iteration */
		OZArray *names = @[ @"Zephyr", @"Objective-Z", @"RTOS" ];
		for (unsigned int i = 0; i < [names count]; i++) {
			OZString *name = [names objectAtIndex:i];
			OZLog("name: %@", name);
		}

		/* Dictionary */
		OZDictionary *scores = @{
			@"alpha" : @100,
			@"beta" : @200,
			@"gamma" : @300,
		};
		OZNumber *alpha = [scores objectForKey:@"alpha"];
		OZLog("alpha score = %@", alpha);

		/* Non-capturing block with enumerateObjectsUsingBlock: */
		[numbers enumerateObjectsUsingBlock:^(id obj, unsigned int idx, BOOL *stop) {
			OZLog("  [%d] = %@", idx, obj);
			*stop = NO;
		}];

		/* Sum using regular for loop (capturing blocks not supported) */
		int sum = 0;
		for (unsigned int i = 0; i < [numbers count]; i++) {
			OZNumber *n = [numbers objectAtIndex:i];
			sum += [n intValue];
		}
		printk("sum = %d\n", sum);

		/* Covariance: assign to generic id array */
		OZArray *generic = names;
		OZLog("generic[0] = %@", [generic objectAtIndex:0]);

		/* Nested arrays */
		OZArray *matrix = @[ @[@1, @2], @[@3, @4] ];
		OZArray *firstRow = [matrix objectAtIndex:0];
		OZLog("matrix[0][1] = %@", [firstRow objectAtIndex:1]);
	}

	printk("=== Transpiled Generics Demo Complete ===\n");
	return 0;
}
