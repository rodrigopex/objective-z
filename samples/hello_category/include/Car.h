
#import <objc/objc.h>
#import <objc/objc.h>
#include <zephyr/kernel.h>

struct color {
	uint8_t r;
	uint8_t g;
	uint8_t b;
};

@interface Car: Object {
	struct color *_color;
	OZString *_model;
	int _throttleLevel;
	int _breakLevel;
}

@property(readonly) struct color *color;

@property(readonly, atomic) OZString *model;
- (Car *)initWithColor:(struct color *)c andModel:(OZString *)model;

- (BOOL)throttleWithLevel:(int)level;

- (BOOL)breakWithLevel:(int)level;

@end
