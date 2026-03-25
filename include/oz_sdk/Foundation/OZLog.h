/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZLog.h
 * @brief Formatted logging with %@ object specifier for transpiled code.
 *
 * OZLog() works like printk() but supports the %@ format specifier
 * to print objects via their -cDescription:maxLength: method.
 */
#pragma once

/**
 * @brief Log a formatted message with optional %@ object support.
 * @param fmt printf-style format string. Use %@ to print an object.
 *        Use %.N@ to limit object description to N decimal digits.
 *
 * Formats into a stack buffer of CONFIG_OBJZ_LOG_BUFFER_SIZE bytes,
 * then outputs via printk with a trailing newline.
 */
void OZLog(const char *fmt, ...);

/**
 * @brief Get the current log format precision for %@ objects.
 * @return Precision (>= 0) if set by OZLog during %.N@ processing, or -1 (default).
 */
int _oz_get_log_precision(void);
