/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOPin.m
 * @brief Base GPIO pin class implementation.
 */
#import <Foundation/OZGPIOPin.h>
#import <Foundation/OZMutableString.h>
#import <objc/objc.h>
#include <string.h>

#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

@implementation OZGPIOPin

- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
	       flags:(gpio_flags_t)flags
{
	self = [super init];
	if (self) {
		_spec = *spec;

		if (!gpio_is_ready_dt(&_spec)) {
			LOG_ERR("GPIO port %s not ready", _spec.port->name);
			[self release];
			return nil;
		}

		int ret = gpio_pin_configure_dt(&_spec, flags);
		if (ret < 0) {
			LOG_ERR("GPIO pin configure failed: %d", ret);
			[self release];
			return nil;
		}
	}
	return self;
}

- (BOOL)isReady
{
	return gpio_is_ready_dt(&_spec);
}

- (id)description
{
	char buf[48];
	snprintk(buf, sizeof(buf), "<%s: %s pin %u>",
		 class_getName(object_getClass(self)),
		 _spec.port->name, _spec.pin);
	return [OZMutableString stringWithCString:buf];
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	return snprintk(buf, maxLen, "<%s: %s pin %u>",
			class_getName(object_getClass(self)),
			_spec.port->name, _spec.pin);
}

@end
