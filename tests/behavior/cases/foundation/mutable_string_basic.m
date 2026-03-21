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
