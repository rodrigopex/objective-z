/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOInput.m
 * @brief GPIO input pin implementation with interrupt callbacks.
 */
#import <Foundation/OZGPIOInput.h>
#import <objc/blocks.h>
#import <objc/objc.h>
#include <zephyr/sys/util.h>

#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

static void __ozgpio_isr_trampoline(const struct device *port, struct gpio_callback *cb,
				    gpio_port_pins_t pins)
{
	struct ozgpio_isr_ctx *ctx = CONTAINER_OF(cb, struct ozgpio_isr_ctx, cb);
	ctx->blockCallback(port, cb, pins);
}

@implementation OZGPIOInput

/**
 * Common GPIO interrupt setup after pin configuration.
 * Registers the callback and configures the interrupt.
 */
- (id)__configureInterrupt:(gpio_callback_handler_t)handler flags:(gpio_flags_t)flags
{
	gpio_init_callback(&_isr.cb, handler, BIT(_spec.pin));

	int ret = gpio_add_callback_dt(&_spec, &_isr.cb);
	if (ret < 0) {
		LOG_ERR("GPIO add callback failed: %d", ret);
		[self release];
		return nil;
	}

	ret = gpio_pin_interrupt_configure_dt(&_spec, flags);
	if (ret < 0) {
		LOG_ERR("GPIO interrupt configure failed: %d", ret);
		gpio_remove_callback(_spec.port, &_isr.cb);
		[self release];
		return nil;
	}

	LOG_DBG("OZGPIOInput configured: %s pin %u", _spec.port->name, _spec.pin);
	return self;
}

- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
	       flags:(gpio_flags_t)flags
	    callback:(gpio_callback_handler_t)callback
{
	self = [super initWithDTSpec:spec flags:GPIO_INPUT];
	if (self) {
		return [self __configureInterrupt:callback flags:flags];
	}
	return self;
}

- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
	       flags:(gpio_flags_t)flags
       blockCallback:(OZGPIOISRBlock)blockCallback
{
	self = [super initWithDTSpec:spec flags:GPIO_INPUT];
	if (self) {
		struct Block_layout *blk = (struct Block_layout *)blockCallback;
		if (!(blk->flags & BLOCK_IS_GLOBAL)) {
			LOG_ERR("OZGPIOInput blockCallback must be a global block "
				"(no captured variables). Use file-scope "
				"variables instead.");
			[self release];
			return nil;
		}

		_isr.blockCallback = blockCallback;
		return [self __configureInterrupt:__ozgpio_isr_trampoline flags:flags];
	}
	return self;
}

- (BOOL)isActive
{
	return gpio_pin_get_dt(&_spec) > 0;
}

- (void)dealloc
{
	if (_isr.cb.handler != NULL) {
		gpio_remove_callback(_spec.port, &_isr.cb);
	}
	[super dealloc];
}

@end
