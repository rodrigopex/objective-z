/*
 * Adapted from: GNUstep libobjc2 — Test/objc_msgSend.m
 * License: MIT
 * Adaptation: Verifies nil messaging returns nil/zero for object-returning
 *             and BOOL-returning methods. Uses ARC-compatible methods only.
 */
#import "OZTestBase.h"

@interface NilMsgTest : OZObject {
	BOOL _initReturnsNil;
	BOOL _isEqualReturnsNo;
}
- (void)testNilMessaging;
- (BOOL)initReturnsNil;
- (BOOL)isEqualReturnsNo;
@end

@implementation NilMsgTest
- (void)testNilMessaging {
	OZObject *nilObj = nil;
	/* nil messaging: [nil init] → nil, [nil isEqual:x] → NO (0) */
	id result = [nilObj init];
	_initReturnsNil = (result == nil);
	BOOL eq = [nilObj isEqual:self];
	_isEqualReturnsNo = (eq == NO);
}
- (BOOL)initReturnsNil {
	return _initReturnsNil;
}
- (BOOL)isEqualReturnsNo {
	return _isEqualReturnsNo;
}
@end
