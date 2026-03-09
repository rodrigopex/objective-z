#import "OZTestBase.h"

@interface Config : OZObject {
	int _level;
}
- (void)setLevel:(int)v;
- (int)level;
@end

@implementation Config
- (void)setLevel:(int)v { _level = v; }
- (int)level { return _level; }
@end
