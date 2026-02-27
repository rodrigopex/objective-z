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

int main(void)
{
	printk("=== Blocks Demo ===\n");

	/* Global block — no captures, immortal */
	IntBlock global = ^{ return 42; };
	printk("Global block: %d\n", global());

	/* Capturing block — captures local variable */
	int value = 99;
	IntBlock capturing = ^{ return value; };
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
	IntBlock inner = ^{ return nested_val; };
	IntBlock outer = ^{ return inner(); };
	void *outer_copy = _Block_copy(outer);
	printk("Nested block: %d\n", ((IntBlock)outer_copy)());
	_Block_release(outer_copy);

	printk("=== Blocks Demo Complete ===\n");
	return 0;
}
