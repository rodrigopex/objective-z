/*
 * Behavioral spec derived from: ObjFW OFDictionaryTests.m
 * ObjFW license: LGPL-3.0-only
 * This test is ORIGINAL CODE — no ObjFW code was copied.
 * Pattern: OZDictionary store and retrieve by key.
 */
/* oz-pool: OZObject=1,OZString=3,OZQ31=1,OZDictionary=1,DictTest=1 */
#import "OZFoundationBase.h"

@interface DictTest : OZObject {
	int _storedVal;
	BOOL _missingNil;
	unsigned int _count;
}
- (void)run;
- (int)storedVal;
- (BOOL)missingNil;
- (unsigned int)count;
@end

@implementation DictTest
- (void)run {
	OZDictionary *d = @{@"key" : @(42)};
	_count = [d count];
	OZQ31 *val = [d objectForKey:@"key"];
	_storedVal = [val intValue];
	id missing = [d objectForKey:@"nope"];
	_missingNil = (missing == nil);
}
- (int)storedVal { return _storedVal; }
- (BOOL)missingNil { return _missingNil; }
- (unsigned int)count { return _count; }
@end
