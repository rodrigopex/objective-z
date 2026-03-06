/* OZ LED sample - standalone ObjC for transpiler testing */
/* No Zephyr headers - pure ObjC */

#import <objc/objc.h>

@protocol OZToggleable
- (void)toggle;
@end

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

@interface OZLed : OZObject <OZToggleable>
{
    int _pin;
    BOOL _state;
}
- (instancetype)initWithPin:(int)pin;
- (void)turnOn;
- (void)turnOff;
- (void)toggle;
- (int)pin;
- (BOOL)state;
@end

@implementation OZLed
- (instancetype)initWithPin:(int)pin {
    self = [super init];
    if (self) {
        _pin = pin;
        _state = NO;
    }
    return self;
}
- (void)turnOn {
    _state = YES;
}
- (void)turnOff {
    _state = NO;
}
- (void)toggle {
    _state = !_state;
}
- (int)pin {
    return _pin;
}
- (BOOL)state {
    return _state;
}
- (void)dealloc {
    [super dealloc];
}
@end

@interface OZRgbLed : OZLed
{
    int _red;
    int _green;
    int _blue;
}
- (instancetype)initWithPin:(int)pin red:(int)r green:(int)g blue:(int)b;
- (void)setRed:(int)r green:(int)g blue:(int)b;
@end

@implementation OZRgbLed
- (instancetype)initWithPin:(int)pin red:(int)r green:(int)g blue:(int)b {
    self = [super initWithPin:pin];
    if (self) {
        _red = r;
        _green = g;
        _blue = b;
    }
    return self;
}
- (void)setRed:(int)r green:(int)g blue:(int)b {
    _red = r;
    _green = g;
    _blue = b;
}
- (void)dealloc {
    [super dealloc];
}
@end
