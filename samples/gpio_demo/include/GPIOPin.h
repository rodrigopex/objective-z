/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOPin.h
 * @brief Base class for GPIO pin wrappers.
 *
 * Holds a copy of the Zephyr gpio_dt_spec and provides common
 * configuration. Use OZGPIOOutput or OZGPIOInput instead of
 * instantiating this class directly.
 */
#pragma once
#import <Foundation/Foundation.h>
#include <zephyr/kernel.h>
#include <zephyr/drivers/gpio.h>

/**
 * @brief Abstract base class for GPIO pins.
 * @headerfile OZGPIOPin.h Foundation/Foundation.h
 * @ingroup objc
 *
 * Stores a struct-copied gpio_dt_spec ivar. Subclasses configure the
 * pin direction (output or input) via their own init methods.
 */
@interface GPIOPin: OZObject
@property(nonatomic, readonly) const struct gpio_dt_spec *spec;

/**
 * @brief Initialize with a devicetree GPIO spec and flags.
 * @param spec Pointer to the devicetree GPIO spec (copied by value).
 * @param flags GPIO configuration flags (ORed with direction by subclass).
 * @return Initialized instance, or nil on configuration failure.
 */
- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec flags:(gpio_flags_t)flags;

/**
 * @brief Check whether the GPIO port device is ready.
 * @return YES if the port device is ready, NO otherwise.
 */
- (BOOL)isReady;

@end
