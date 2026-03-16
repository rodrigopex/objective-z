/* oz-pool: OZObject=1,OZNumber=4,OZDictionary=2 */
#import "OZFoundationBase.h"

@interface DictTest : OZObject
- (unsigned int)literalCount;
- (int)valueForKey;
- (BOOL)missingKeyNil;
@end

@implementation DictTest
- (unsigned int)literalCount {
	OZDictionary *d = @{@"a": @(1), @"b": @(2)};
	unsigned int c = [d count];
	[d release];
	return c;
}
- (int)valueForKey {
	OZDictionary *d = @{@"x": @(99)};
	OZNumber *n = [d objectForKey:@"x"];
	int v = [n intValue];
	[d release];
	return v;
}
- (BOOL)missingKeyNil {
	OZDictionary *d = @{@"a": @(1)};
	id obj = [d objectForKey:@"z"];
	[d release];
	return obj == nil;
}
@end
