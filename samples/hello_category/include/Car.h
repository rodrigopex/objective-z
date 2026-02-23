
#import <objc/objc.h>
#include <zephyr/kernel.h>

struct color {
	uint8_t r;
	uint8_t g;
	uint8_t b;
};

@interface Car: Object {
	struct color *_color;
	NXConstantString *_model;
	int _throttleLevel;
	int _breakLevel;
}

@property(readonly) struct color *color;

@property(readonly, atomic) NXConstantString *model;
- (Car *)initWithColor:(struct color *)c andModel:(NXConstantString *)model;

- (BOOL)throttleWithLevel:(int)level;

- (BOOL)breakWithLevel:(int)level;

@end
