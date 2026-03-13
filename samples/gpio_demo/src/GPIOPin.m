/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOPin.m
 * @brief Base GPIO pin class implementation.
 */
#import "GPIOPin.h"

@implementation GPIOPin {
	const struct gpio_dt_spec *_spec;
}

@synthesize spec = _spec;

- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec flags:(gpio_flags_t)flags
{
	self = [super init];
	if (self) {
		_spec = spec;

		if (!gpio_is_ready_dt(_spec)) {
			printk("GPIO port not ready\n"); //, _spec->port->name);
			return nil;
		}

		int ret = gpio_pin_configure_dt(_spec, flags);
		if (ret < 0) {
			printk("GPIO pin configure failed: %d\n", ret);
			return nil;
		}
	}
	return self;
}

- (BOOL)isReady
{
	return gpio_is_ready_dt(_spec);
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	return snprintk(buf, maxLen, "<%s: %s pin %u>", "GPIOPin", _spec->port->name, _spec->pin);
}

@end
