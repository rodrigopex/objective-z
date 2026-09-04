/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Introspection and reflection demo (#226).
 *
 * Every answer here is computed from tables the transpiler generated:
 * a superclass chain indexed by class id, one conformance bitmap per
 * protocol, and one const record per selector named by a @selector(...).
 * All of it is const, so it lives in flash and costs no RAM, and none of
 * it is emitted for a construct this file does not use.
 *
 * Class identity -- [Foo class], [obj class], -isMemberOfClass: -- needs
 * no table at all: `Class` is the class_id every object already carries.
 * -isKindOfClass: and -conformsToProtocol: need CONFIG_OBJZ_INTROSPECTION;
 * @selector, SEL, -respondsToSelector: and -performSelector: need
 * CONFIG_OBJZ_REFLECTION. Both default to y.
 */

#import <Foundation/Foundation.h>

@protocol Switchable
- (void)toggle;
@end

@interface Light : OZObject <Switchable> {
	int _on;
}
- (void)toggle;
- (int)isOn;
- (id)describeTo:(id)peer;
@end

@implementation Light

/* Returns void, which is what makes it performable. It matters here
 * because this file performs through a `SEL` held in a local, so nothing
 * can tell which selector reaches that site and every selector named by a
 * `@selector(...)` has to fit the uniform wrapper shape: at most two
 * object-typed arguments, returning void or an object. An `int`-returning
 * `-toggle` was refused at transpile time with exactly that message --
 * which is the intended behaviour, not a limitation worked around here.
 * Were every perform site given a literal, only the literals named there
 * would carry the requirement (`Program::needs_perform_wrapper`). */
- (void)toggle
{
	_on = !_on;
}

- (int)isOn
{
	return _on;
}

/* Object in, object out, so it fits the uniform wrapper shape that
 * -performSelector:withObject: needs. */
- (id)describeTo:(id)peer
{
	OZLog("Light describing itself to a peer");
	return peer;
}

@end

/* Conforms to Switchable only through its superclass, and never declares
 * the protocol itself -- so a conformance check that looked only at
 * declared protocols would get this one wrong. */
@interface DimmableLight : Light
@end

@implementation DimmableLight
@end

/* Neither a Light nor Switchable, so the negative answers below are real
 * rather than vacuous. */
@interface Fan : OZObject
@end

@implementation Fan
@end

/* `OZLog` appends its own newline (`printk("%s\n", buf)`) and truncates
 * any single `%s` argument at 31 characters, so labels here are short and
 * carry no trailing newline of their own. */
static void report(const char *what, int ok)
{
	OZLog("%s=%s", what, ok ? "yes" : "no");
}

int main(void)
{
	OZLog("=== Reflection Demo ===");

	DimmableLight *dim = [DimmableLight alloc];
	Fan *fan = [Fan alloc];
	Light *missing = nil;

	/* Class identity: no table, no option. Two classes are two
	 * distinct constants, and an instance reports its own class rather
	 * than the one it was declared as. */
	report("classes-differ", [Light class] != [DimmableLight class]);
	report("own-class", [dim class] == [DimmableLight class]);
	/* -isMemberOfClass: is exact, so a DimmableLight is not a member of
	 * Light however much it inherits from it. */
	report("member-own", [dim isMemberOfClass:[DimmableLight class]]);
	report("member-super", [dim isMemberOfClass:[Light class]]);

	/* Introspection: the superclass chain and the conformance bitmap. */
	report("kind-own", [dim isKindOfClass:[DimmableLight class]]);
	report("kind-super", [dim isKindOfClass:[Light class]]);
	report("kind-root", [dim isKindOfClass:[OZObject class]]);
	report("kind-unrelated", [fan isKindOfClass:[Light class]]);
	/* DimmableLight never declares Switchable -- it conforms only
	 * through Light. */
	report("conforms-inherited", [dim conformsToProtocol:@protocol(Switchable)]);
	report("conforms-unrelated", [fan conformsToProtocol:@protocol(Switchable)]);

	/* Reflection: the selector records. `toggle` is a SEL in a local,
	 * so this exercises the value path and not only a literal at the
	 * call site. */
	SEL toggle = @selector(toggle);
	report("responds", [dim respondsToSelector:toggle]);
	report("responds-not", [fan respondsToSelector:toggle]);
	report("responds-keyword", [dim respondsToSelector:@selector(describeTo:)]);

	[dim performSelector:toggle];
	OZLog("performed-void on=%d", [dim isOn]);
	report("performed-arg",
	       [dim performSelector:@selector(describeTo:) withObject:fan] == (id)fan);

	/* A message to nil answers rather than faulting -- including
	 * against the root class, which holds only because a nil receiver's
	 * class matches nothing at all. */
	report("nil-member", [missing isMemberOfClass:[Light class]]);
	report("nil-kind-root", [missing isKindOfClass:[OZObject class]]);
	report("nil-responds", [missing respondsToSelector:toggle]);
	report("nil-performs-nil", [missing performSelector:toggle] == nil);

	OZLog("=== Reflection Demo complete ===");
	return 0;
}
