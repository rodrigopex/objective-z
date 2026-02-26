/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * ObjC boxed literals and collection literals demo.
 * Compiled without ARC — uses MRR with @autoreleasepool.
 */

#import <objc/objc.h>
#import <objc/OZAutoreleasePool.h>

int main(void)
{
	OZLog("=== ObjC Literals Demo ===");

	@autoreleasepool {
		/* Boolean literals */
		OZNumber *yes = @YES;
		OZNumber *no = @NO;
		OZLog("@YES = %@", yes);
		OZLog("@NO  = %@", no);

		/* Small integer cache (0..15 are singletons) */
		OZNumber *zero = @0;
		OZNumber *fifteen = @15;
		OZLog("@0  = %@", zero);
		OZLog("@15 = %@", fifteen);

		/* Heap-allocated integer */
		OZNumber *big = @1000;
		OZLog("@1000 = %@", big);

		/* Double literal */
		OZNumber *pi = @3.14;
		OZLog("@3.14 = %@", pi);

		/* Array literal */
		OZArray *arr = @[ @"hello", @42 ];
		OZLog("array = %@", arr);
		OZLog("arr[0] = %@", [arr objectAtIndex:0]);
		OZLog("arr[1] = %@", [arr objectAtIndex:1]);

		/* Dictionary literal */
		OZDictionary *dict = @{@"key" : @"value"};
		OZLog("dict = %@", dict);
		OZLog("dict[@\"key\"] = %@", [dict objectForKey:@"key"]);
	}

	OZLog("=== Demo complete ===");
	return 0;
}
