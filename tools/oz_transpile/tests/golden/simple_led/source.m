#import <Foundation/OZObject.h>

@protocol OZToggleable
- (void)toggle;
@end

@interface OZLed : OZObject <OZToggleable> {
        int _pin;
        BOOL _state;
}
- (instancetype)initWithPin:(int)pin;
- (instancetype)init;
- (void)toggle;
- (void)turnOn;
- (int)pin;
@end

@implementation OZLed

- (instancetype)initWithPin:(int)pin {
        [super init];
        _pin = pin;
        return self;
}

- (instancetype)init {
        return self;
}

- (void)toggle {
        _state = !_state;
}

- (void)turnOn {
        _state = 1;
}

- (int)pin {
        return _pin;
}

@end
