/*
 * Copyright (c) 2012-2014 Wind River Systems, Inc.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#import "Car+Maintenance.h"
#import <Foundation/Foundation.h>
#import <objc/objc.h>
#import <assert.h>

int main(void)
{
	Car *myCar = [[Car alloc] initWithColor:&(struct color){255, 255, 0}
				       andModel:@"Honda Civic"];

	myCar->_plate = 0xAABBCC;
	oz_assert(myCar->_plate == 0xAABBCC);
	oz_assert([myCar throttleWithLevel:50] == YES);
	oz_assert([myCar breakWithLevel:20] == NO);
	oz_assert([myCar throttleWithLevel:0] == YES);
	oz_assert([myCar breakWithLevel:20] == YES);
	oz_assert([myCar milage] != 0);
	oz_assert([[myCar model] isEqual:@"Honda Civic"]);

	OZLog("All assertions passed");
	return 0;
}
