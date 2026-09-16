/* oz-pool: OZObject=1 */
#import "OZFoundationBase.h"

@interface MutableStringTest : OZObject {
	OZMutableString *_ms;
}
/* setup methods — store result in _ms ivar */
- (void)buildFromCString;
- (void)buildFromOZString;
- (void)buildWithCapacity;
- (void)buildAndAppendCString;
- (void)buildAndAppendString;
- (void)buildAndAppendGrow;
- (void)buildAndSetString;
- (void)buildAndSetStringNil;
- (void)buildAndReinitialise;
/* mutators on a receiver that never ran an initialiser (#542) */
- (void)appendCStringToUninitialised;
- (void)setStringOnUninitialised;
- (void)setStringNilOnUninitialised;
/* query methods — read from _ms ivar */
- (const char *)result;
- (unsigned int)resultLength;
/* OZString method tests (no ivar needed) */
- (BOOL)hasPrefixTrue;
- (BOOL)hasSuffixTrue;
- (BOOL)isEqualToStringTrue;
@end

@implementation MutableStringTest

- (void)buildFromCString
{
	_ms = [[OZMutableString alloc] initWithCString:"hello"];
}

- (void)buildFromOZString
{
	OZString *src = @"world";
	_ms = [[OZMutableString alloc] initWithString:src];
}

- (void)buildWithCapacity
{
	_ms = [[OZMutableString alloc] initWithCapacity:64];
	[_ms appendCString:"reserved"];
}

- (void)buildAndAppendCString
{
	_ms = [[OZMutableString alloc] initWithCString:"hello"];
	[_ms appendCString:", world"];
}

- (void)buildAndAppendString
{
	_ms = [[OZMutableString alloc] initWithCString:"hello"];
	OZString *suffix = @", world";
	[_ms appendString:suffix];
}

- (void)buildAndAppendGrow
{
	_ms = [[OZMutableString alloc] initWithCString:"a"];
	[_ms appendCString:"bcdefghijklmnop"];
	[_ms appendCString:"qrstuvwxyz"];
}

- (void)buildAndSetString
{
	_ms = [[OZMutableString alloc] initWithCString:"old content"];
	OZString *replacement = @"new";
	[_ms setString:replacement];
}

- (void)buildAndSetStringNil
{
	_ms = [[OZMutableString alloc] initWithCString:"content"];
	[_ms setString:nil];
}

/*
 * The same object initialised twice. `-initWithCString:` mallocs into
 * `_data`, and before #405 it did so without freeing what was already
 * there -- so the first buffer leaked and `-dealloc` could not make up
 * for it, since by then `_data` names only the second. Observable under
 * LeakSanitizer (`just test-behavior --check-leaks`); the content
 * assertion below only says the reinitialisation itself works.
 */
- (void)buildAndReinitialise
{
	_ms = [[OZMutableString alloc] initWithCString:"first"];
	[_ms initWithCString:"second"];
}

/*
 * Three mutators reached on a receiver that never ran an initialiser
 * (#542). `+alloc` memsets the slab slot, so `_capacity == 0` and
 * `_data == NULL` at entry, and nothing in the class requires an
 * initialiser before a mutator.
 *
 * A case that starts from an initialised instance cannot reach any of
 * this, which is why these three exist rather than an extra assertion on
 * one of the cases above. Before the fix the first two did not fail --
 * they *hung*, because the capacity-doubling loop seeded at `_capacity`
 * spun on `0 * 2 == 0` forever. That is why this coverage lives in the
 * behavior corpus: `tests/behavior/conftest.py` bounds the run at 60s and
 * `tests/tools/compile_and_run.py` bounds the binary at 30s, so a
 * regression is reported as a timeout rather than wedging the suite.
 */
- (void)appendCStringToUninitialised
{
	_ms = [OZMutableString alloc];
	[_ms appendCString:"grown from nothing"];
}

- (void)setStringOnUninitialised
{
	_ms = [OZMutableString alloc];
	OZString *replacement = @"assigned from nothing";
	[_ms setString:replacement];
}

/*
 * The nil branch of `-setString:`. Its argument check was always present
 * and always correct; what it did not check was the *receiver's* state,
 * so `((char *)_data)[0] = '\0'` wrote through a NULL `_data`. There is
 * no content to assert here -- `-cString` returns `_data` verbatim and
 * this receiver has no buffer -- so the claim is the length and the fact
 * that the process survives to read it.
 */
- (void)setStringNilOnUninitialised
{
	_ms = [OZMutableString alloc];
	[_ms setString:nil];
}

- (const char *)result
{
	return [_ms cString];
}

- (unsigned int)resultLength
{
	return [_ms length];
}

- (BOOL)hasPrefixTrue
{
	OZString *s = @"hello world";
	OZString *prefix = @"hello";
	return [s hasPrefix:prefix];
}

- (BOOL)hasSuffixTrue
{
	OZString *s = @"hello world";
	OZString *suffix = @"world";
	return [s hasSuffix:suffix];
}

- (BOOL)isEqualToStringTrue
{
	OZString *a = @"hello";
	OZString *b = @"hello";
	return [a isEqualToString:b];
}

@end
