/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZLog.h
 * @brief Formatted logging with %@ object specifier for transpiled code.
 *
 * OZLog() works like printk() but supports the %@ format specifier
 * to print objects via their -getDescription:maxLength: method.
 */
#pragma once

/**
 * @brief Log a formatted message with optional %@ object support.
 * @param fmt printf-style format string. Use %@ to print an object.
 *        Use %.N@ to limit object description to N decimal digits.
 *
 * Formats into a buffer of CONFIG_OBJZ_LOG_BUFFER_SIZE bytes (int,
 * range 32 to 1024, default 128) and then outputs it via printk with a
 * trailing newline. The buffer is an automatic array, so it is spent on
 * the stack of the calling thread -- every thread that logs, not one.
 *
 * A line longer than that does not overflow and does not fault: writing
 * stops at the boundary, so the tail of the line is dropped silently and
 * the fields that disappear are the last ones. A %@ straddling the
 * boundary lands mid-description, `-getDescription:maxLength:` having
 * been handed only the bytes that remained -- the same silent-truncation
 * contract it carries everywhere else, reached through the buffer rather
 * than through %.N@.
 */
void OZLog(const char *fmt, ...);

/**
 * @brief Get the current log format precision for %@ objects.
 * @return Precision (>= 0) if set by OZLog during %.N@ processing, or -1 (default).
 */
int _oz_get_log_precision(void);
