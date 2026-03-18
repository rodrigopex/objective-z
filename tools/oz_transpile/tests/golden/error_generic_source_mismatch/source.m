#import <Foundation/Foundation.h>

@protocol DataProto
- (int)processValue:(int)value;
@end

@protocol SensorProto
- (int)readValue;
@end

@interface Sensor : OZObject <SensorProto>
- (int)readValue;
@end

@implementation Sensor
- (int)readValue {
    return 42;
}
@end

int main(void) {
    Sensor *s = [[Sensor alloc] init];
    OZArray<id<DataProto>> *arr = @[ s ];
    return 0;
}
