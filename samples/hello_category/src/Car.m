#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include "Car.h"

@implementation Car

@synthesize color = _color;
@synthesize model = _model;

- (Car *)initWithColor:(struct color *)c andModel:(OZString *)model
{
	self = [super init];

	if (self) {
		_color = c;
		_model = model;
		_throttleLevel = 0;
		_breakLevel = 0;
	}

	return self;
}

- (BOOL)throttleWithLevel:(int)level
{
	if (level < 0 || level > 100) {
		return NO;
	}
	if (self->_breakLevel > 0 && level > 0) {
		// Cannot throttle while braking
		return NO;
	}

	self->_throttleLevel = level;

	return YES;
}

- (BOOL)breakWithLevel:(int)level

{
	if (level < 0 || level > 100) {
		return NO;
	}
	if (self->_throttleLevel > 0 && level > 0) {
		return NO;
	}

	self->_breakLevel = level;

	return YES;
}

@end
