/*
 * Behavioral spec derived from: ObjFW OFArrayTests.m
 * ObjFW license: LGPL-3.0-only
 * This test is ORIGINAL CODE — no ObjFW code was copied.
 * Pattern: OZArray count, objectAtIndex, out-of-bounds.
 */
/* oz-pool: OZObject=1,OZQ31=3,OZArray=1,ArrIdxTest=1 */
#import "OZFoundationBase.h"

@interface ArrIdxTest : OZObject {
	unsigned int _count;
	int _first;
	int _last;
	BOOL _oobNil;
}
- (void)run;
- (unsigned int)count;
- (int)first;
- (int)last;
- (BOOL)oobNil;
@end

@implementation ArrIdxTest
- (void)run {
	OZArray *arr = @[@(10), @(20), @(30)];
	_count = [arr count];
	OZQ31 *f = [arr objectAtIndex:0];
	_first = [f intValue];
	OZQ31 *l = [arr objectAtIndex:2];
	_last = [l intValue];
	id oob = [arr objectAtIndex:99];
	_oobNil = (oob == nil);
}
- (unsigned int)count { return _count; }
- (int)first { return _first; }
- (int)last { return _last; }
- (BOOL)oobNil { return _oobNil; }
@end
