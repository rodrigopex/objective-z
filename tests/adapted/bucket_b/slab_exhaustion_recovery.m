/*
 * Adapted from: tests/objc-reference/runtime/memory/src/main.c
 * Adaptation: Removed objc_lookupClass introspection.
 *             Pattern: exhaust slab, free one, allocate again.
 */
/* oz-pool: Slot=2,ExhaustTest=1 */
#import "OZTestBase.h"

@interface Slot : OZObject {
	int _id;
}
- (void)setId:(int)i;
- (int)id;
@end

@implementation Slot
- (void)setId:(int)i { _id = i; }
- (int)id { return _id; }
@end

@interface ExhaustTest : OZObject {
	int _recoveryOk;
}
- (void)run;
- (int)recoveryOk;
@end

@implementation ExhaustTest
- (void)run {
	Slot *b = nil;
	{
		/* Exhaust 2-block slab */
		Slot *a = [Slot alloc];
		b = [Slot alloc];
	}
	/* ARC scope-exit frees 'a' — allocate again should succeed */
	Slot *c = [Slot alloc];
	_recoveryOk = (c != nil) ? 1 : 0;
}
- (int)recoveryOk {
	return _recoveryOk;
}
@end
