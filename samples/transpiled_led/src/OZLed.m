/* OZLed - Standalone ObjC source for transpiler demo */

@protocol OZToggleable
- (void)toggle;
@end

@interface OZObject
{
    int _refcount;
}
- (instancetype)init;
- (void)dealloc;
@end

@implementation OZObject
- (instancetype)init {
    return self;
}
- (void)dealloc {
}
@end

@interface OZLed : OZObject <OZToggleable>
{
    int _pin;
    int _state;
}
- (instancetype)initWithPin:(int)pin;
- (void)turnOn;
- (void)turnOff;
- (void)toggle;
- (int)pin;
- (int)state;
@end

@implementation OZLed
- (instancetype)initWithPin:(int)pin {
    self = [super init];
    if (self) {
        _pin = pin;
        _state = 0;
    }
    return self;
}
- (void)turnOn {
    _state = 1;
}
- (void)turnOff {
    _state = 0;
}
- (void)toggle {
    _state = !_state;
}
- (int)pin {
    return _pin;
}
- (int)state {
    return _state;
}
- (void)dealloc {
    [super dealloc];
}
@end
