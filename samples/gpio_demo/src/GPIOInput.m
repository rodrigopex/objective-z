/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOInput.m
 * @brief GPIO input pin implementation with interrupt callbacks.
 */
#import "GPIOInput.h"

@implementation GPIOInput {
	struct gpio_callback cb;
}

- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
		flags:(gpio_flags_t)flags
	blockCallback:(void (^)(const struct device *port, struct gpio_callback *cb,
				gpio_port_pins_t pins))blockCallback
{
	self = [super initWithDTSpec:spec flags:GPIO_INPUT];

	gpio_init_callback(&self->cb, (gpio_callback_handler_t)blockCallback, BIT(spec.pin));

	int ret = gpio_add_callback_dt(spec, &self->cb);
	if (ret < 0) {
		OZLog("GPIO add callback failed: %d", ret);
		return nil;
	}

	ret = gpio_pin_interrupt_configure_dt(spec, flags);
	if (ret < 0) {
		OZLog("GPIO interrupt configure failed: %d", ret);
		gpio_remove_callback(spec->port, &self->cb);
		return nil;
	}

	OZLog("OZGPIOInput configured: %s pin %u", spec->port->name, spec->pin);

	return self;
}

- (BOOL)isActive
{
	return gpio_pin_get_dt(super.spec) > 0;
}

- (void)dealloc
{
	if (self->cb.handler != NULL) {
		gpio_remove_callback(super.spec->port, &self->cb);
	}
	[super dealloc];
}

@end
