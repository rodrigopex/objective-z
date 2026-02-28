/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZLog.h
 * @brief Formatted logging with %@ object specifier.
 *
 * OZLog() works like printk() but supports the %@ format specifier
 * to print Objective-C objects via their -cStr method.
 */
#pragma once

/**
 * @brief Log a formatted message with optional %@ object support.
 * @param fmt printf-style format string. Use %@ to print an ObjC object.
 *
 * Formats into a stack buffer of CONFIG_OBJZ_LOG_BUFFER_SIZE bytes,
 * then outputs via printk with a trailing newline.
 */
void OZLog(const char *fmt, ...);
