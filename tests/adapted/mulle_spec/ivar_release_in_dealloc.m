/*
 * Behavioral spec derived from: mulle-objc runtime lifecycle patterns
 * mulle-objc license: BSD-3-Clause
 * This test is ORIGINAL CODE inspired by mulle-objc's two-phase teardown.
 * Pattern: strong ivar released when owner is deallocated.
 */
/* oz-pool: Owned=1,Owner=1,IvarRelTest=1 */
#import "OZTestBase.h"

@interface Owned : OZObject
@end

@implementation Owned
@end

@interface Owner : OZObject {
	Owned *_child;
}
- (void)setChild:(Owned *)c;
@end

@implementation Owner
- (void)setChild:(Owned *)c {
	_child = c;
}
@end

@interface IvarRelTest : OZObject {
	int _canReallocOwned;
}
- (void)run;
- (int)canReallocOwned;
@end

@implementation IvarRelTest
- (void)run {
	{
		Owner *o = [Owner alloc];
		Owned *c = [Owned alloc];
		[o setChild:c];
	}
	/* ARC scope-exit releases owner, dealloc releases child ivar */
	/* If child was released, 1-block slab is free */
	Owned *proof = [Owned alloc];
	_canReallocOwned = (proof != nil) ? 1 : 0;
}
- (int)canReallocOwned {
	return _canReallocOwned;
}
@end
