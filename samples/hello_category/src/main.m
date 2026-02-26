/*
 * Copyright (c) 2012-2014 Wind River Systems, Inc.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#import "Car+Maintenance.h"
#import <objc/objc.h>
#import <assert.h>

int main(void)
{
	Car *myCar = [[Car alloc] initWithColor:&(struct color){255, 255, 0}
				       andModel:@"Honda Civic"];

	assert([myCar throttleWithLevel:50] == YES);
	assert([myCar breakWithLevel:20] == NO);
	assert([myCar throttleWithLevel:0] == YES);
	assert([myCar breakWithLevel:20] == YES);
	assert([myCar milage] != 0);

	[myCar dealloc];

	OZLog("All assertions passed");
	return 0;
}
