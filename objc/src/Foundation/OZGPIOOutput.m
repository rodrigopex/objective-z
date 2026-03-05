/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOOutput.m
 * @brief GPIO output pin implementation.
 */
#import <Foundation/OZGPIOOutput.h>
#import <objc/objc.h>

#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

@implementation OZGPIOOutput

- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
	       flags:(gpio_flags_t)flags
{
	self = [super initWithDTSpec:spec flags:(GPIO_OUTPUT | flags)];
	if (self) {
		LOG_DBG("OZGPIOOutput configured: %s pin %u",
			_spec.port->name, _spec.pin);
	}
	return self;
}

- (BOOL)isActive
{
	return _active;
}

- (void)setActive:(BOOL)active
{
	gpio_pin_set_dt(&_spec, active ? 1 : 0);
	_active = active;
}

- (void)toggle
{
	gpio_pin_toggle_dt(&_spec);
	_active = !_active;
}

@end
