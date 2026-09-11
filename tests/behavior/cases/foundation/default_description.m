/* `-getDescription:maxLength:` has a useful default (#354).
 *
 * `OZObject`'s implementation is what every class inherits until it writes
 * its own, and it used to be `return 0;` -- an undocumented no-op. So `%@`
 * on any class without its own description produced an empty field, which
 * is indistinguishable from a description that really is empty and from a
 * formatting bug. It now writes `<ClassName: 0xADDRESS>`, the shape
 * Objective-C's `-description` defaults to.
 *
 * Exercised through a **direct send**, not through `OZLog`: the `%@` path
 * is the same call, and `src/OZLog.c` needs `zephyr/sys/printk.h`, which
 * the host corpus cannot link. (`%@` end to end is covered on target.)
 *
 * The address is not reproducible, so nothing here asserts the whole
 * string -- only the prefix, the terminator, and that the reported length
 * matches what was written.
 *
 * `run_bounded` is the one that matters most. This default writes into
 * whatever buffer `OZLog` hands it, mid-format, with only the remaining
 * space as the bound, so it is asked for every limit from 0 to past the
 * full description -- truncating in the middle of the class name and in
 * the middle of the hex digits -- and checked never to write beyond what
 * it reports. Under `just test-behavior --sanitize=address` an overrun is
 * reported rather than merely mis-measured.
 */
/* oz-pool: Plain=1,Custom=1 */
#import "OZTestBase.h"

/*
 * Hand-rolled rather than `<string.h>`: the corpus harness dumps the
 * Clang AST for this file with no libc sysroot, so `memset` and friends
 * are undeclared there ("ISO C99 and later do not support implicit
 * function declarations"). Nothing else in this corpus calls a libc
 * string function, so there was no precedent to copy -- and a case that
 * needs none is the better shape anyway.
 */
static void fill(char *buf, char c, int n)
{
	int i = 0;

	for (i = 0; i < n; i++) {
		buf[i] = c;
	}
}

static int length(const char *s)
{
	int n = 0;

	while (s[n] != '\0') {
		n++;
	}
	return n;
}

static int starts_with(const char *s, const char *prefix)
{
	int i = 0;

	while (prefix[i] != '\0') {
		if (s[i] != prefix[i]) {
			return 0;
		}
		i++;
	}
	return 1;
}

static int same(const char *a, const char *b)
{
	int i = 0;

	while (a[i] != '\0' && a[i] == b[i]) {
		i++;
	}
	return a[i] == b[i];
}

@interface Plain : OZObject
@end

@implementation Plain
@end

/* A class that writes its own: the default must not override it. */
@interface Custom : OZObject
@end

@implementation Custom
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen
{
	const char *s = "custom!";
	size_t n = (size_t)length(s);
	size_t i = 0;

	if (n > maxLen) {
		n = maxLen;
	}
	for (i = 0; i < n; i++) {
		buf[i] = s[i];
	}
	return (int)n;
}
@end

int run_default_names_the_class(void)
{
	char buf[64];
	Plain *p = [Plain alloc];
	int n = 0;

	fill(buf, 0, (int)sizeof(buf));
	n = [p getDescription:buf maxLength:sizeof(buf) - 1];

	return (n == length(buf)) && starts_with(buf, "<Plain: 0x") &&
	       (buf[length(buf) - 1] == '>') && (length(buf) > 11);
}

int run_own_description_wins(void)
{
	char buf[64];
	Custom *c = [Custom alloc];
	int n = 0;

	fill(buf, 0, (int)sizeof(buf));
	n = [c getDescription:buf maxLength:sizeof(buf) - 1];
	return (n == 7) && same(buf, "custom!");
}

int run_bounded(void)
{
	Plain *p = [Plain alloc];
	size_t limit = 0;

	for (limit = 0; limit <= 32; limit++) {
		char buf[64];
		int n = 0;

		fill(buf, '#', (int)sizeof(buf));
		n = [p getDescription:buf maxLength:limit];
		if (n < 0 || (size_t)n > limit) {
			return 0;
		}
		/* Nothing beyond what it reported may have been touched. */
		if (buf[n] != '#') {
			return 0;
		}
	}
	return 1;
}
