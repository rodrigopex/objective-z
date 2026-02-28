/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.m
 * @brief Lightweight generics demo for Objective-Z.
 *
 * Demonstrates compile-time type-safe collections using ObjC
 * lightweight generics on Zephyr RTOS.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>
#import <objc/blocks.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

int main(void)
{
	printk("=== Generics Demo ===\n");

	/* Typed array of numbers — subscript returns OZNumber * */
	OZArray<OZNumber *> *numbers = @[ @10, @20, @30 ];
	OZNumber *first = numbers[0];
	OZLog("numbers[0] = %@", first);
	OZLog("numbers count = %d", [numbers count]);

	/* Typed array of strings — for-in with typed variable */
	OZArray<OZString *> *names = @[ @"Zephyr", @"Objective-Z", @"RTOS" ];
	for (OZString *name in names) {
		OZLog("name: %@", name);
	}

	/* Typed dictionary — keyed subscript returns typed value */
	OZDictionary<OZString *, OZNumber *> *scores = @{
		@"alpha" : @100,
		@"beta" : @200,
		@"gamma" : @300,
	};
	OZNumber *alpha = scores[@"alpha"];
	OZLog("alpha score = %@", alpha);

	/* Typed block param in enumerateObjectsUsingBlock: */
	__block int sum = 0;
	[numbers enumerateObjectsUsingBlock:^(OZNumber *obj, unsigned int idx, BOOL *stop) {
	  sum += [obj intValue];
	  *stop = NO;
	}];
	printk("sum = %d\n", sum);

	/* Covariance: OZArray<OZString *> assignable to OZArray<id> */
	OZArray<id> *generic = names;
	OZLog("generic[0] = %@", generic[0]);

	/* Nested generics: array of arrays */
	OZArray<OZNumber *> *row1 = @[ @1, @2 ];
	OZArray<OZNumber *> *row2 = @[ @3, @4 ];
	OZArray<OZArray<OZNumber *> *> *matrix = @[ row1, row2 ];
	OZArray<OZNumber *> *firstRow = matrix[0];
	OZLog("matrix[0][1] = %@", firstRow[1]);

	printk("=== Generics Demo Complete ===\n");
	return 0;
}
