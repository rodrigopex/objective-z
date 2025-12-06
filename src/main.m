/*
 * Copyright (c) 2012-2014 Wind River Systems, Inc.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#import <objc/objc.h>
#include <zephyr/kernel.h>

@interface MyFirstObject: Object
- (void)greet;
+ (void)greet;
@end

@implementation MyFirstObject
- (void)greet
{
	printk("Hello, world from object\n");
}
+ (void)greet
{
	printk("Hello, world from class\n");
}
@end

int main(void)
{
	// Send a "greet" method to the Object class
	MyFirstObject *hello = [[MyFirstObject alloc] init];

	[MyFirstObject greet];
	[hello greet];
	return 0;
}
