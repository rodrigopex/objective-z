/* OZLed - Standalone ObjC source for transpiler demo */

#import "OZLed.h"

@implementation OZObject
- (instancetype)init
{
	return self;
}
- (void)dealloc
{
}
@end

@implementation OZHelper
- (instancetype)initWithValue:(int)value andHelper:(OZHelper *)helper
{
	self = [super init];
	if (self) {
		_value = value;
		_next = helper;
	}
	return self;
}
- (int)value
{
	return _value;
}
@end

@implementation OZLed
- (instancetype)initWithPin:(int)pin
{
	self = [super init];
	if (self) {
		_pin = pin;
		_state = 0;
	}
	return self;
}
- (void)turnOn
{
	_state = 1;
}
- (void)turnOff
{
	_state = 0;
}
- (void)toggle
{
	_state = !_state;
}
- (int)pin
{
	return _pin;
}
- (int)state
{
	return _state;
}
- (void)setHelper:(OZHelper *)helper
{
	_helper = helper;
}
- (void)dealloc
{
	[super dealloc];
}
@end
