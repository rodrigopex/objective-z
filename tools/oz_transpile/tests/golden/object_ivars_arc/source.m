#import <Foundation/OZObject.h>

@interface Sensor : OZObject {
        int _value;
}
- (int)value;
@end

@implementation Sensor

- (int)value {
        return _value;
}

@end

@interface Controller : OZObject {
        Sensor *_sensor;
        int _count;
}
- (void)setSensor:(Sensor *)s;
@end

@implementation Controller

- (void)setSensor:(Sensor *)s {
        _sensor = s;
}

@end
