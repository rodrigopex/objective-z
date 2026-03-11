/* OZLed - ObjC header for transpiler demo */
#pragma once

#import <Foundation/OZObject.h>

@protocol OZToggleable
- (void)toggle;
@end

@interface OZHelper : OZObject
{
    int _value;
    OZHelper *_next;
}
- (instancetype)initWithValue:(int)value andHelper:(OZHelper *)helper;
- (int)value;
@end

@interface OZLed : OZObject <OZToggleable>
{
    int _pin;
    int _state;
    OZHelper *_helper;
}
- (instancetype)initWithPin:(int)pin;
- (void)turnOn;
- (void)turnOff;
- (void)toggle;
- (int)pin;
- (int)state;
- (void)setHelper:(OZHelper *)helper;
@end
