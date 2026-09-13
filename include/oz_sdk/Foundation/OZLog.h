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
 * @brief The precision the `%@` currently being rendered was written with.
 * @return Precision (>= 0) while `OZLog` is processing a `%.N@`, else -1.
 *
 * Carries no `get`, deliberately: `get` marks a method or function that
 * writes through a caller's pointer (`-getBytes:length:range:`,
 * `-getDescription:maxLength:`), which #413 settled as the rule. This
 * returns its value, so `get` would be a lie -- the same reasoning that
 * keeps `-usedBytes` and `-cString` free of it. #417 renamed it: the old
 * spelling was public while carrying the *internal* `_oz_` prefix, and
 * misused `get`, in one name. `docs/STATUS.md` records what it was.
 *
 * Valid only inside a `-getDescription:maxLength:` called from a `%@`
 * conversion. `OZLog` sets it immediately before the dispatch and clears it
 * immediately after, so a caller that reads it from anywhere else sees -1.
 */
int oz_log_precision(void);
