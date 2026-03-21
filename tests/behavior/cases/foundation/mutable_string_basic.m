/* oz-pool: OZObject=1 */
/* oz-heap */
#import "OZFoundationBase.h"

@interface MutableStringTest : OZObject
- (const char *)initFromCString;
- (unsigned int)initFromCStringLength;
- (const char *)initFromOZString;
- (const char *)initWithCapacity;
- (const char *)appendCString;
- (const char *)appendString;
- (const char *)appendGrow;
- (const char *)setStringReplace;
- (const char *)setStringNil;
- (BOOL)hasPrefixTrue;
- (BOOL)hasSuffixTrue;
- (BOOL)isEqualToStringTrue;
@end

@implementation MutableStringTest

- (const char *)initFromCString
{
	OZMutableString *s = [[OZMutableString alloc] initWithCString:"hello"];
	return [s cString];
}

- (unsigned int)initFromCStringLength
{
	OZMutableString *s = [[OZMutableString alloc] initWithCString:"hello"];
	return [s length];
}

- (const char *)initFromOZString
{
	OZString *src = @"world";
	OZMutableString *s = [[OZMutableString alloc] initWithString:src];
	return [s cString];
}

- (const char *)initWithCapacity
{
	OZMutableString *s = [[OZMutableString alloc] initWithCapacity:64];
	[s appendCString:"reserved"];
	return [s cString];
}

- (const char *)appendCString
{
	OZMutableString *s = [[OZMutableString alloc] initWithCString:"hello"];
	[s appendCString:", world"];
	return [s cString];
}

- (const char *)appendString
{
	OZMutableString *s = [[OZMutableString alloc] initWithCString:"hello"];
	OZString *suffix = @", world";
	[s appendString:suffix];
	return [s cString];
}

- (const char *)appendGrow
{
	OZMutableString *s = [[OZMutableString alloc] initWithCString:"a"];
	/* Append enough to trigger buffer growth (initial capacity is 16) */
	[s appendCString:"bcdefghijklmnop"];
	[s appendCString:"qrstuvwxyz"];
	return [s cString];
}

- (const char *)setStringReplace
{
	OZMutableString *s = [[OZMutableString alloc] initWithCString:"old content"];
	OZString *replacement = @"new";
	[s setString:replacement];
	return [s cString];
}

- (const char *)setStringNil
{
	OZMutableString *s = [[OZMutableString alloc] initWithCString:"content"];
	[s setString:nil];
	return [s cString];
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
