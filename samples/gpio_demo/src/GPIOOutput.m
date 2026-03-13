/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file GPIOOutput.m
 * @brief GPIO output pin implementation.
 */
#import "GPIOOutput.h"

@implementation GPIOOutput {
	BOOL _active;
}

- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
	       flags:(gpio_flags_t)flags
{
	self = [super initWithDTSpec:spec flags:(GPIO_OUTPUT | flags)];
	if (self) {
		_active = NO;
	}
	return self;
}

- (BOOL)isActive
{
	return _active;
}

- (void)setActive:(BOOL)active
{
	_active = active;
	gpio_pin_set_dt(super.spec, active ? 1 : 0);
}

- (void)toggle
{
	_active = !_active;
	gpio_pin_toggle_dt(super.spec);
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	return snprintk(buf, maxLen, "<%s: %s pin %u, active=%d>",
			"GPIOOutput", super.spec->port->name, super.spec->pin, _active);
}

@end
