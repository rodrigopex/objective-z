/* OZLed - ObjC source for transpiler demo */

#import "OZLed.h"

int printk(const char *fmt, ...);

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

- (void)dealloc
{
	printk("Helper dealloc %d\n", _value);
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
	printk("Led dealloc %d\n", _pin);
	[super dealloc];
}
@end
