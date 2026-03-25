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

	/* Q31 fixed point numbers literals */
	OZQ31 *a = @10;
	OZQ31 *b = @25002031;
	OZLog("a = %@, b = %@, a + b = %@", a, b, [a add:b]);
	OZLog("a = %@, b = %@, a - b = %@", a, b, [a sub:b]);
	OZLog("a = %@, b = %@, a * b = %@", a, b, [a mul:b]);
	OZLog("a = %@, b = %@, a / b = %.10@", a, b, [a div:b]);

	/* Precision demo: 1/3 has full Q31 precision */
	OZQ31 *one = @1;
	OZQ31 *three = @3;
	OZQ31 *third = [one div:three];
	OZLog("1/3 default %%@:   %@", third);
	OZLog("1/3 with %%.4@:    %.4@", third);
	OZLog("1/3 with %%.10@:   %.10@", third);
	OZLog("1/3 with %%.14@:   %.14@", third);

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

	OZLog("=== Demo complete ===");
	return 0;
}
