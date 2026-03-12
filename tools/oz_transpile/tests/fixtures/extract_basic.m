/* Copyright 2024 Rodrigo Peixoto */

#import <Foundation/Foundation.h>
#include <zephyr/kernel.h>

@interface OZBlinky: OZObject
- (id)initWithPin:(int)pin;
- (void)toggle;
@end

@implementation OZBlinky

/* Initialize the blinky LED */
- (id)initWithPin:(int)pin
{
	self = [super init];
	if (self) {
		self->_pin = pin;
	}
	return self;
}

/* Toggle the LED state */
- (void)toggle
{
	/* TODO: implement GPIO toggle */
}

@end
