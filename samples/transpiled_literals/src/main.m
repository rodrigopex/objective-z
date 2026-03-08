/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * ObjC string, array, and dictionary literals demo.
 * Transpiled to pure C via oz_transpile.
 */

#import <Foundation/Foundation.h>

int main(void)
{
	OZLog("=== ObjC Literals Demo ===");

	@autoreleasepool {
		/* String literals */
		OZString *greeting = @"hello";
		OZLog("greeting = %@", greeting);

		/* Array literal */
		OZArray *arr = @[ @"hello", @"world" ];
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
