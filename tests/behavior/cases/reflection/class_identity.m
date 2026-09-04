/* Class identity: [Foo class], [obj class], -isMemberOfClass: (#226).
 *
 * `Class` is the class_id every object already carries, so all three are
 * a constant or a bitfield read and none of them needs an option set.
 *
 * Before #226 this file could not have existed: `+class` is declared once
 * on the root class, so `[Widget class]` resolved to `OZObject_class_cls()`
 * -- dropping the receiver's class, and defined nowhere, so it failed at
 * link time with an undefined symbol. */
#import "OZTestBase.h"

@interface Widget : OZObject
@end
@implementation Widget
@end

@interface Gadget : Widget
@end
@implementation Gadget
@end

@interface Identity : OZObject
+ (int)check;
@end

@implementation Identity
+ (int)check
{
	Widget *w = [Widget alloc];
	Gadget *g = [Gadget alloc];
	Widget *none = nil;
	int bits = 0;

	/* Two classes are two distinct constants. */
	if ([Widget class] != [Gadget class]) {
		bits = bits | 1;
	}
	/* An instance reports its own class, not its declared one. */
	if ([g class] == [Gadget class]) {
		bits = bits | 2;
	}
	/* -isMemberOfClass: is exact: a Gadget is not a member of Widget,
	 * however much it inherits from it. */
	if ([w isMemberOfClass:[Widget class]]) {
		bits = bits | 4;
	}
	if (![g isMemberOfClass:[Widget class]]) {
		bits = bits | 8;
	}
	/* A message to nil answers the way Objective-C does. `Nil` itself
	 * cannot be named in Objective-C source (see OZObject.h), so this
	 * asserts the consequence instead: a nil receiver's class matches
	 * nothing at all, not even the class it was declared as. */
	if (![none isMemberOfClass:[Widget class]]) {
		bits = bits | 16;
	}
	if ([none class] != [Widget class] && [none class] != [Gadget class]) {
		bits = bits | 32;
	}

	return bits;
}
@end
