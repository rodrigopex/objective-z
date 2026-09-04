/* @selector, SEL, -respondsToSelector: and -performSelector: (#226).
 *
 * Needs CONFIG_OBJZ_REFLECTION, which defaults to y.
 *
 * -tick is implemented by exactly one class, which is the case that made
 * `Program::is_dynamically_dispatched` grow a reflection clause: without
 * it no OZ_PROTOCOL_SEND_tick is generated, and the uniform-shape wrapper
 * a SEL calls through would reference a function that does not exist. */
#import "OZTestBase.h"

static int g_ticks = 0;

@interface Counter : OZObject
- (void)tick;
- (id)echo:(id)thing;
@end

@implementation Counter
- (void)tick
{
	g_ticks = g_ticks + 1;
}
- (id)echo:(id)thing
{
	return thing;
}
@end

@interface Silent : OZObject
@end
@implementation Silent
@end

@interface Selectors : OZObject
+ (int)check;
@end

@implementation Selectors
+ (int)check
{
	Counter *c = [Counter alloc];
	Silent *s = [Silent alloc];
	Counter *none = nil;
	/* A SEL in a local, so this exercises a value and not only a
	 * literal at the call site. */
	SEL tick = @selector(tick);
	int bits = 0;

	if ([c respondsToSelector:tick]) {
		bits = bits | 1;
	}
	if (![s respondsToSelector:tick]) {
		bits = bits | 2;
	}
	/* A keyword selector -- the name tree-sitter exposes no node for. */
	if ([c respondsToSelector:@selector(echo:)]) {
		bits = bits | 4;
	}
	if (![none respondsToSelector:tick]) {
		bits = bits | 8;
	}

	[c performSelector:tick];
	[c performSelector:@selector(tick)];
	if (g_ticks == 2) {
		bits = bits | 16;
	}
	if ([c performSelector:@selector(echo:) withObject:c] == (id)c) {
		bits = bits | 32;
	}
	/* A void selector hands back nil, not whatever was in the return
	 * register -- unlike real Objective-C, which yields a garbage id. */
	if ([c performSelector:tick] == nil) {
		bits = bits | 64;
	}
	if ([none performSelector:tick] == nil) {
		bits = bits | 128;
	}

	return bits;
}
@end
