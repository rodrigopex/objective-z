/* -isKindOfClass: and -conformsToProtocol: (#226).
 *
 * Both need CONFIG_OBJZ_INTROSPECTION, which defaults to y and which
 * tests/tools/oz_static_build.py passes so the corpus runs the
 * configuration a real build gets.
 *
 * Inheritance on both axes is the point. A one-class hierarchy would pass
 * with an `==` in place of the ancestry walk, and a protocol every
 * conformer declared directly would pass without the superclass chain
 * `Program::class_conforms_to` walks. */
#import "OZTestBase.h"

@protocol Togglable
- (int)toggle;
@end

@interface Switchable : OZObject <Togglable>
- (int)toggle;
@end
@implementation Switchable
- (int)toggle
{
	return 1;
}
@end

/* Conforms only through its superclass, and never declares the protocol. */
@interface DimmerSwitch : Switchable
@end
@implementation DimmerSwitch
@end

@interface Unrelated : OZObject
@end
@implementation Unrelated
@end

@interface Kinds : OZObject
+ (int)check;
@end

@implementation Kinds
+ (int)check
{
	DimmerSwitch *d = [DimmerSwitch alloc];
	Unrelated *u = [Unrelated alloc];
	DimmerSwitch *none = nil;
	int bits = 0;

	if ([d isKindOfClass:[DimmerSwitch class]]) {
		bits = bits | 1;
	}
	/* One step up the chain. */
	if ([d isKindOfClass:[Switchable class]]) {
		bits = bits | 2;
	}
	/* Two steps up, to the root. */
	if ([d isKindOfClass:[OZObject class]]) {
		bits = bits | 4;
	}
	if (![u isKindOfClass:[Switchable class]]) {
		bits = bits | 8;
	}
	if (![none isKindOfClass:[Switchable class]]) {
		bits = bits | 16;
	}
	/* Conformance inherited rather than declared. */
	if ([d conformsToProtocol:@protocol(Togglable)]) {
		bits = bits | 32;
	}
	if (![u conformsToProtocol:@protocol(Togglable)]) {
		bits = bits | 64;
	}
	if (![none conformsToProtocol:@protocol(Togglable)]) {
		bits = bits | 128;
	}

	return bits;
}
@end
