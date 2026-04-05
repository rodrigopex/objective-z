/*
 * Behavioral spec derived from: Apple ARC documentation
 * This test is ORIGINAL CODE — no Apple code was copied.
 * Pattern: strong reference between objects; verify both usable.
 */
/* oz-pool: NodeA=1,NodeB=1,CycleTest=1 */
#import "OZTestBase.h"

@interface NodeA : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation NodeA
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
@end

@interface NodeB : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation NodeB
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
@end

@interface CycleTest : OZObject {
	int _aTag;
	int _bTag;
}
- (void)run;
- (int)aTag;
- (int)bTag;
@end

@implementation CycleTest
- (void)run {
	NodeA *a = [NodeA alloc];
	NodeB *b = [NodeB alloc];
	[a setTag:1];
	[b setTag:2];
	_aTag = [a tag];
	_bTag = [b tag];
}
- (int)aTag { return _aTag; }
- (int)bTag { return _bTag; }
@end
