/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.m
 * @brief Transpiled generics demo for Objective-Z.
 *
 * Demonstrates typed collections, subscript access, for-in loops,
 * blocks, and covariance — all transpiled to pure C via oz_transpile.
 */
#import <Foundation/Foundation.h>
#include <zephyr/kernel.h>

int main(void)
{
	printk("=== Generics Demo ===\n");

	@autoreleasepool {
		/* Typed array of numbers — subscript returns OZQ31 * */
		OZArray<OZQ31 *> *numbers = @[ @10, @20, @30 ];
		OZQ31 *first = numbers[0];
		OZLog("numbers[0] = %@", first);
		OZLog("numbers count = %d", [numbers count]);

		/* Typed array of strings — for-in with typed variable */
		OZArray<OZString *> *names = @[ @"Zephyr", @"Objective-Z", @"RTOS" ];
		for (OZString *name in names) {
			OZLog("name: %@", name);
		}

		/* Typed dictionary — keyed subscript returns typed value */
		OZDictionary<OZString *, OZQ31 *> *scores = @{
			@"alpha" : @100,
			@"beta" : @200,
			@"gamma" : @300,
		};
		OZQ31 *alpha = scores[@"alpha"];
		OZLog("alpha score = %@", alpha);

		/* Typed block param in enumerateObjectsUsingBlock: */
		__block int sum = 0;
		[numbers enumerateObjectsUsingBlock:^(id obj, unsigned int idx, BOOL *stop) {
		  sum += [(OZQ31 *)obj intValue];
		  *stop = NO;
		}];
		printk("sum = %d\n", sum);

		/* Covariance: OZArray<OZString *> assignable to OZArray<id> */
		OZArray<id> *generic = names;
		OZLog("generic[0] = %@", generic[0]);

		/* Nested generics: array of arrays */
		OZArray<OZArray<OZQ31 *> *> *matrix = @[ @[@1, @2], @[@3, @4] ];
		OZArray<OZQ31 *> *firstRow = matrix[0];
		OZLog("matrix[0][1] = %@", firstRow[1]);
	}

	printk("=== Generics Demo Complete ===\n");
	return 0;
}
