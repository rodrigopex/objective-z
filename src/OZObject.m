/* Root class implementation for OZ transpiler samples. */

#import <Foundation/OZObject.h>

/*
 * `<ClassName: 0xADDRESS>` into `buf`, the default every class inherits
 * from `-cDescription:maxLength:` until it writes its own (#354).
 *
 * Hand-rolled rather than `snprintf`: this runs on targets with no stdio
 * linked, and the existing descriptions in this SDK all write bytes
 * directly for the same reason (`OZString` memcpy's, `OZQ31` calls
 * `_oz_q31_to_str`).
 *
 * `static inline`, and that is load-bearing rather than a performance
 * hint. File-scope code ahead of an `@implementation` is spliced into the
 * *generated header* for this origin, which every Foundation translation
 * unit includes -- so a plain `static` definition is unused in all but
 * one of them, and Zephyr builds with `-Werror`. It failed
 * `-Wunused-function` on every sample whose generated set includes a file
 * that does not call it, which the single-purpose samples this was first
 * tried on happened not to (`samples/mem_demo` did). `static inline`
 * emits no code where it is unused and does not warn, which is why the
 * whole PAL is written that way.
 *
 * Truncates rather than overflowing, and truncates *silently*, which is
 * the contract every other implementation here already has: the return
 * value is what was written, and `maxLen` is a hard bound. A description
 * cut short is a cosmetic problem; one byte past the end of a log buffer
 * is not.
 */
#if OZ_DEFAULT_DESCRIPTION
static inline int _oz_write_default_description(const char *class_name, unsigned long address,
					       char *buf, size_t maxLen)
{
	static const char hex[] = "0123456789abcdef";
	size_t pos = 0;
	int shift = 0;
	int leading = 1;

	if (buf == NULL || maxLen == 0) {
		return 0;
	}
	if (pos < maxLen) {
		buf[pos++] = '<';
	}
	while (*class_name != '\0' && pos < maxLen) {
		buf[pos++] = *class_name++;
	}
	if (pos < maxLen) {
		buf[pos++] = ':';
	}
	if (pos < maxLen) {
		buf[pos++] = ' ';
	}
	if (pos < maxLen) {
		buf[pos++] = '0';
	}
	if (pos < maxLen) {
		buf[pos++] = 'x';
	}
	/* Most significant nibble first, skipping leading zeroes -- but
	 * never all of them, so a null address still prints as `0x0`. */
	for (shift = (int)(sizeof(unsigned long) * 8) - 4; shift >= 0; shift -= 4) {
		unsigned long nibble = (address >> shift) & 0xful;

		if (leading && nibble == 0ul && shift > 0) {
			continue;
		}
		leading = 0;
		if (pos < maxLen) {
			buf[pos++] = hex[nibble];
		}
	}
	if (pos < maxLen) {
		buf[pos++] = '>';
	}
	return (int)pos;
}
#endif /* OZ_DEFAULT_DESCRIPTION */

@implementation OZObject
+ (instancetype)alloc
{
	return nil;
}
- (instancetype)init
{
	return self;
}
- (void)dealloc
{
}
- (BOOL)isEqual:(id)anObject
{
	return self == anObject;
}
- (int)cDescription:(char *)buf maxLength:(size_t)maxLen
{
#if OZ_DEFAULT_DESCRIPTION
	return _oz_write_default_description(oz_static_class_name(self),
					     (unsigned long)self, buf, maxLen);
#else
	(void)buf;
	(void)maxLen;
	return 0;
#endif
}
@end
