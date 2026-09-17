/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Class-side resolution, on target: `self` and `super` inside a `+` method,
 * and the inherited `+new` (#534, #535, #539).
 *
 * This sample exists for the half of the fix no host gate can see. `[self
 * alloc]` is lowered to `PXAltimeter_oz_alloc()`, which draws from a
 * `k_mem_slab` that `pools.rs` has to have *counted the site for* -- and
 * before #534 it could not, because `self` is not a class name, so
 * `ever_slab_allocated` answered no and no slab was emitted at all. A
 * transpile-and-compile gate passes either way; what the missing slab
 * produces is a factory that returns **nil** on its first call. So the
 * assertions that matter here are that the ivars came back with the values
 * the factory was given, on a board, out of a real slab.
 *
 * It is `px-app`'s `src/challenges/PXAltimeter.m` with WA-009 and WA-010
 * taken back off: the canonical `[[self alloc] init…]` factory, and
 * `[super familyDepth] + 1` reaching past a level that does not redeclare
 * it.
 */

#import <Foundation/OZObject.h>
#include <zephyr/kernel.h>

/* ── Level 1 ──────────────────────────────────────────────────────── */

@interface PXSensorBase : OZObject {
	int _id;
}
+ (int)familyDepth;
- (instancetype)initWithId:(int)sensorId;
- (int)sensorId;
@end

@implementation PXSensorBase

+ (int)familyDepth
{
	return 1;
}

- (instancetype)init
{
	self = [super init];
	_id = -1;
	return self;
}

- (instancetype)initWithId:(int)sensorId
{
	self = [super init];
	_id = sensorId;
	return self;
}

- (int)sensorId
{
	return _id;
}

@end

/*
 * Level 2, declaring nothing. It is the whole point of the `+familyDepth`
 * probe below: a class-side `[super familyDepth]` has to walk *past* this
 * level to reach level 1, which is what #535 reports as broken -- and what
 * it was, though not for the reason the issue gives. `find_defining_class`
 * always walked the chain; a class-side super send arrived asking for an
 * *instance* method, so it missed at every level including the immediate
 * parent.
 */
@interface PXMid : PXSensorBase
@end

@implementation PXMid
@end

/* ── Level 3 ──────────────────────────────────────────────────────── */

@interface PXAltimeter : PXMid {
	int _ceiling;
}
+ (instancetype)altimeterWithId:(int)sensorId ceiling:(int)ceiling;
+ (int)familyDepth;
- (instancetype)initWithId:(int)sensorId ceiling:(int)ceiling;
- (int)ceiling;
@end

@implementation PXAltimeter

/*
 * The canonical Cocoa factory. `self` here is the class whose
 * `@implementation` encloses this body -- a generated class method takes no
 * receiver parameter, so there is no dynamic receiver to resolve against,
 * and `[Subclass altimeterWithId:…]` would still allocate a `PXAltimeter`.
 * What #534 buys is that the canonical spelling compiles; the inheritance
 * it is written for remains outside the static subset.
 */
+ (instancetype)altimeterWithId:(int)sensorId ceiling:(int)ceiling
{
	return [[self alloc] initWithId:sensorId ceiling:ceiling];
}

+ (int)familyDepth
{
	return [super familyDepth] + 1;
}

- (instancetype)initWithId:(int)sensorId ceiling:(int)ceiling
{
	/* Instance-side `super` across the same silent level -- it already
	 * worked, and it is here so a regression on that side fails this
	 * sample rather than only the Rust suite. */
	self = [super initWithId:sensorId];
	_ceiling = ceiling;
	return self;
}

- (int)ceiling
{
	return _ceiling;
}

@end

int main(void)
{
	printk("=== Class Side Demo ===\n");

	PXAltimeter *a = [PXAltimeter altimeterWithId:7 ceiling:9000];
	if (a == nil) {
		/* The nil-at-runtime failure this sample exists to catch: a
		 * class whose `[self alloc]` site was never counted gets no
		 * `k_mem_slab`, and every allocation answers nil. */
		printk("factory returned nil -- PXAltimeter has no slab\n");
		return 1;
	}
	printk("factory id=%d ceiling=%d\n", [a sensorId], [a ceiling]);

	printk("familyDepth=%d\n", [PXAltimeter familyDepth]);

	/*
	 * The inherited `+new`, declared on OZObject with no body and
	 * resolved at the send site to *this* class's allocator and *this*
	 * class's `-init` -- which is why `id` comes back as -1 rather than
	 * the 0 a memset would leave.
	 */
	PXSensorBase *plain = [PXSensorBase new];
	if (plain == nil) {
		printk("+new returned nil -- PXSensorBase has no slab\n");
		return 1;
	}
	printk("new id=%d\n", [plain sensorId]);

	printk("=== Demo complete ===\n");
	return 0;
}
