/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * ObjC boxed literals and collection literals demo.
 * Compiled without ARC — uses MRR with @autoreleasepool.
 */

#import <objc/objc.h>
#import <objc/OZAutoreleasePool.h>
#include <zephyr/kernel.h>

int main(void)
{
	printk("=== ObjC Literals Demo ===\n");

	@autoreleasepool {
		/* Boolean literals */
		OZNumber *yes = @YES;
		OZNumber *no = @NO;
		printk("@YES boolValue=%d\n", [yes boolValue]);
		printk("@NO boolValue=%d\n", [no boolValue]);

		/* Small integer cache (0..15 are singletons) */
		OZNumber *zero = @0;
		OZNumber *fifteen = @15;
		printk("@0 intValue=%d\n", [zero intValue]);
		printk("@15 intValue=%d\n", [fifteen intValue]);

		/* Heap-allocated integer */
		OZNumber *big = @1000;
		printk("@1000 intValue=%d\n", [big intValue]);

		/* Double literal (no %f on Cortex-M, use intValue) */
		OZNumber *pi = @3.14;
		printk("@3.14 intValue=%d\n", [pi intValue]);

		/* Array literal */
		OZArray *arr = @[ @"hello", @42 ];
		printk("array count=%u\n", [arr count]);
		printk("arr[0]=%s\n", [(NXConstantString *)[arr objectAtIndex:0] cStr]);
		printk("arr[1]=%d\n", [(OZNumber *)[arr objectAtIndex:1] intValue]);

		/* Dictionary literal */
		OZDictionary *dict = @{@"key" : @"value"};
		printk("dict count=%u\n", [dict count]);
		printk("dict[@\"key\"]=%s\n",
		       [(NXConstantString *)[dict objectForKey:@"key"] cStr]);
	}

	printk("=== Demo complete ===\n");
	return 0;
}
