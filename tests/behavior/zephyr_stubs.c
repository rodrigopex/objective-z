/* Host-side definitions for the Zephyr symbols transpiled code calls.
 *
 * The headers under `include/zephyr_stubs/` declare these; a host build that
 * actually *links* needs them defined somewhere, and on target Zephyr's own
 * kernel provides them. Link this alongside the generated objects.
 *
 * Kept a real implementation rather than a no-op: `printk` is how every
 * sample reports what it did, so a host run that discards it proves nothing.
 */
#include <stdarg.h>
#include <stdio.h>

/* Declared by <zephyr/sys/printk.h>, and often redeclared by transpiled
 * sources themselves so their Clang AST dump resolves without Zephyr. */
void printk(const char *fmt, ...)
{
	va_list args;

	va_start(args, fmt);
	(void)vprintf(fmt, args);
	va_end(args);
}
