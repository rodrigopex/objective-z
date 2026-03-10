/* @synchronized sample - tests OZLock RAII transpilation */

#import <objc/objc.h>

@interface OZObject
{
    int _refcount;
}
- (instancetype)init;
- (void)dealloc;
- (instancetype)retain;
- (void)release;
@end

@implementation OZObject
- (instancetype)init {
    return self;
}
- (void)dealloc {
}
- (instancetype)retain {
    _refcount++;
    return self;
}
- (void)release {
    _refcount--;
    if (_refcount == 0) {
        [self dealloc];
    }
}
@end

@interface Counter : OZObject
{
    int _count;
}
- (void)increment;
- (int)getCount;
@end

@implementation Counter
- (void)increment {
    @synchronized(self) {
        _count++;
    }
}
- (int)getCount {
    int val;
    @synchronized(self) {
        val = _count;
    }
    return val;
}
- (void)dealloc {
    [super dealloc];
}
@end
