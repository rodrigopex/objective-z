/*
 * Copyright (c) 2012-2014 Wind River Systems, Inc.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#import <objc/objc.h>

@interface MyFirstObject: Object

- (void)greet;

+ (void)greet;

@end

@implementation MyFirstObject

- (void)greet
{
	OZLog("Hello, world from object");
}

+ (void)greet
{
	OZLog("Hello, world from class");
}

@end

int main(void)
{
	/* Send a "greet" method to the Object class */
	[MyFirstObject greet];

	MyFirstObject *hello = [[MyFirstObject alloc] init];

	[hello greet];

	[hello dealloc];

	return 0;
}
