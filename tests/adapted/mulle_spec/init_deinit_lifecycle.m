/*
 * Behavioral spec derived from: mulle-objc runtime lifecycle patterns
 * mulle-objc license: BSD-3-Clause
 * This test is ORIGINAL CODE inspired by mulle-objc's lifecycle conventions.
 * Pattern: alloc → init → use → release follows deterministic order.
 */
/* oz-pool: LifecycleObj=1,LifecycleTest=1 */
#import "OZTestBase.h"

@interface LifecycleObj : OZObject {
	int _stage;
}
- (instancetype)initWithStage;
- (int)stage;
- (void)advance;
@end

@implementation LifecycleObj
- (instancetype)initWithStage {
	_stage = 1;
	return self;
}
- (int)stage {
	return _stage;
}
- (void)advance {
	_stage = _stage + 1;
}
@end

@interface LifecycleTest : OZObject {
	int _initStage;
	int _useStage;
}
- (void)run;
- (int)initStage;
- (int)useStage;
@end

@implementation LifecycleTest
- (void)run {
	LifecycleObj *obj = [[LifecycleObj alloc] initWithStage];
	_initStage = [obj stage];
	[obj advance];
	_useStage = [obj stage];
}
- (int)initStage {
	return _initStage;
}
- (int)useStage {
	return _useStage;
}
@end
