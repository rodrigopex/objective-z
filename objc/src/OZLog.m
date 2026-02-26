/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZLog.m
 * @brief Formatted logging with %@ object specifier.
 *
 * ObjC implementation using -description for %@ format specifiers.
 * Wrapped in @autoreleasepool to clean up temporary strings.
 */
#import <objc/objc.h>
#include <stdarg.h>
#include <zephyr/sys/printk.h>

/**
 * @brief Get a C-string description from an ObjC object.
 *
 * Calls -description then -cStr on the result. Falls back to the
 * class name if -description returns nil, or "(nil)" for nil objects.
 */
static const char *__ozlog_describe(id obj)
{
	if (obj == nil) {
		return "(nil)";
	}

	id desc = [obj description];
	if (desc) {
		return [desc cStr];
	}

	const char *name = class_getName(object_getClass(obj));
	return name ? name : "<unknown>";
}

void OZLog(const char *fmt, ...)
{
	@autoreleasepool {
		char buf[CONFIG_OBJZ_LOG_BUFFER_SIZE];
		va_list args;
		int pos = 0;
		const int max = (int)sizeof(buf) - 1;

		va_start(args, fmt);

		const char *p = fmt;
		while (*p && pos < max) {
			if (*p != '%') {
				buf[pos++] = *p++;
				continue;
			}

			p++; /* skip '%' */
			if (*p == '\0') {
				break;
			}

			/* %% → literal '%' */
			if (*p == '%') {
				buf[pos++] = '%';
				p++;
				continue;
			}

			/* %@ → object description */
			if (*p == '@') {
				id obj = va_arg(args, id);
				const char *s = __ozlog_describe(obj);
				while (*s && pos < max) {
					buf[pos++] = *s++;
				}
				p++;
				continue;
			}

			/* Regular format specifier: collect into spec[] */
			char spec[32];
			int si = 0;
			spec[si++] = '%';

			/* Flags: -, +, space, #, 0 */
			while (*p == '-' || *p == '+' || *p == ' ' ||
			       *p == '#' || *p == '0') {
				if (si < (int)sizeof(spec) - 2) {
					spec[si++] = *p;
				}
				p++;
			}

			/* Width */
			while (*p >= '0' && *p <= '9') {
				if (si < (int)sizeof(spec) - 2) {
					spec[si++] = *p;
				}
				p++;
			}

			/* Precision */
			if (*p == '.') {
				if (si < (int)sizeof(spec) - 2) {
					spec[si++] = *p;
				}
				p++;
				while (*p >= '0' && *p <= '9') {
					if (si < (int)sizeof(spec) - 2) {
						spec[si++] = *p;
					}
					p++;
				}
			}

			/* Length modifier */
			int is_long = 0;
			int is_long_long = 0;
			if (*p == 'l') {
				if (si < (int)sizeof(spec) - 2) {
					spec[si++] = *p;
				}
				p++;
				is_long = 1;
				if (*p == 'l') {
					if (si < (int)sizeof(spec) - 2) {
						spec[si++] = *p;
					}
					p++;
					is_long_long = 1;
				}
			} else if (*p == 'h') {
				if (si < (int)sizeof(spec) - 2) {
					spec[si++] = *p;
				}
				p++;
				if (*p == 'h') {
					if (si < (int)sizeof(spec) - 2) {
						spec[si++] = *p;
					}
					p++;
				}
			} else if (*p == 'z') {
				if (si < (int)sizeof(spec) - 2) {
					spec[si++] = *p;
				}
				p++;
			}

			/* Conversion character */
			char conv = *p;
			if (si < (int)sizeof(spec) - 1) {
				spec[si++] = *p;
			}
			p++;
			spec[si] = '\0';

			char tmp[32];
			tmp[0] = '\0';

			switch (conv) {
			case 'd':
			case 'i':
				if (is_long_long) {
					snprintk(tmp, sizeof(tmp), spec,
						 va_arg(args, long long));
				} else if (is_long) {
					snprintk(tmp, sizeof(tmp), spec,
						 va_arg(args, long));
				} else {
					snprintk(tmp, sizeof(tmp), spec,
						 va_arg(args, int));
				}
				break;
			case 'u':
			case 'x':
			case 'X':
			case 'o':
				if (is_long_long) {
					snprintk(tmp, sizeof(tmp), spec,
						 va_arg(args, unsigned long long));
				} else if (is_long) {
					snprintk(tmp, sizeof(tmp), spec,
						 va_arg(args, unsigned long));
				} else {
					snprintk(tmp, sizeof(tmp), spec,
						 va_arg(args, unsigned int));
				}
				break;
			case 's': {
				const char *s = va_arg(args, const char *);
				snprintk(tmp, sizeof(tmp), "%s",
					 s ? s : "(null)");
				break;
			}
			case 'c':
				snprintk(tmp, sizeof(tmp), spec,
					 va_arg(args, int));
				break;
			case 'p':
				snprintk(tmp, sizeof(tmp), spec,
					 va_arg(args, void *));
				break;
			default:
				break;
			}

			const char *t = tmp;
			while (*t && pos < max) {
				buf[pos++] = *t++;
			}
		}

		buf[pos] = '\0';
		va_end(args);

		printk("%s\n", buf);
	}
}
