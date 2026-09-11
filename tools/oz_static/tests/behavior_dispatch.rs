// SPDX-License-Identifier: Apache-2.0
//
// behavior_dispatch.rs - OZ-092: port of the Python-pipeline "dispatch"
// behavior fixtures (tests/behavior/cases/dispatch/*.m + *_test.c) to the
// static-subset spike. Each Python fixture pair is a class declaration
// (.m) plus a hand-written Unity _test.c calling the generated API
// directly; here the class declarations and the assertions are folded
// into one source string with a `main()` that `printf`s the values under
// test, and the Rust test asserts the exact stdout -- same shape as
// end_to_end_behavior.rs. Uses the real `OZObject` (`common::ozobject_src`)
// as the root class, and `alloc`/`init`/inherited or overridden methods
// are exercised through ordinary `[receiver selector]` sends -- the
// static bar resolves the receiver's declared type and dispatches to the
// correct implementation at compile time, so there's no need to
// reproduce the oracle's raw `(struct Parent *)` casts by hand.

mod common;
use common::{compile_and_run, ozobject_src};

#[test]
fn class_method_dispatch() {
    // Ported from tests/behavior/cases/dispatch/class_method.m /
    // class_method_test.c: a class method (`+version`) dispatches and
    // returns its value, with no instance ever created.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Factory : OZObject
+ (int)version;
@end

@implementation Factory
+ (int)version {
	return 42;
}
@end

#include <stdio.h>

int main(void) {
	printf(\"version=%d\\n\", [Factory version]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "class_method_dispatch");
    assert_eq!(stdout, "version=42\n");
}

#[test]
fn inherited_method_dispatch() {
    // Ported from inherited_method.m / inherited_method_test.c: Car
    // declares no methods of its own -- `[c speed]` must resolve up the
    // hierarchy to Vehicle's implementation.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Vehicle : OZObject {
	int _speed;
}
- (instancetype)init;
- (int)speed;
@end

@implementation Vehicle
- (instancetype)init {
	self = [super init];
	_speed = 60;
	return self;
}
- (int)speed {
	return _speed;
}
@end

@interface Car : Vehicle
@end
@implementation Car
@end

#include <stdio.h>

int main(void) {
	Car *c = [Car alloc];
	c = [c init];
	printf(\"speed=%d\\n\", [c speed]);
	[c release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "inherited_method_dispatch");
    assert_eq!(stdout, "speed=60\n");
}

#[test]
fn method_override_dispatch() {
    // Ported from method_override.m / method_override_test.c: Dog
    // overrides Animal's `sound`; a Dog must call its own, and Animal
    // instances must be unaffected by the subclass's override.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Animal : OZObject
- (int)sound;
@end
@implementation Animal
- (int)sound {
	return 1;
}
@end

@interface Dog : Animal
- (int)sound;
@end
@implementation Dog
- (int)sound {
	return 2;
}
@end

#include <stdio.h>

int main(void) {
	Dog *d = [Dog alloc];
	printf(\"dog_sound=%d\\n\", [d sound]);
	[d release];

	Animal *a = [Animal alloc];
	printf(\"animal_sound=%d\\n\", [a sound]);
	[a release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "method_override_dispatch");
    assert_eq!(stdout, "dog_sound=2\nanimal_sound=1\n");
}

#[test]
fn send_routes_correct_dispatch() {
    // Ported from send_routes_correct.m / send_routes_correct_test.c: a
    // plain instance method call routes to the correct implementation and
    // observably mutates the receiver's own ivar. `alloc` zero-initializes
    // storage, so `_spoken` starts at 0 with no explicit init needed.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Speaker : OZObject {
	int _spoken;
}
- (void)speak;
- (int)spoken;
@end

@implementation Speaker
- (void)speak {
	_spoken = 1;
}
- (int)spoken {
	return _spoken;
}
@end

#include <stdio.h>

int main(void) {
	Speaker *s = [Speaker alloc];
	printf(\"before=%d\\n\", [s spoken]);
	[s speak];
	printf(\"after=%d\\n\", [s spoken]);
	[s release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "send_routes_correct_dispatch");
    assert_eq!(stdout, "before=0\nafter=1\n");
}

#[test]
fn super_calls_parent_dispatch() {
    // Ported from super_calls_parent.m / super_calls_parent_test.c: a
    // three-level `[super init]` chain (Child -> Base -> OZObject), each
    // level setting its own ivar -- both must be observable afterward.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Base : OZObject {
	int _baseVal;
}
- (instancetype)init;
- (int)baseVal;
@end

@implementation Base
- (instancetype)init {
	self = [super init];
	_baseVal = 10;
	return self;
}
- (int)baseVal {
	return _baseVal;
}
@end

@interface Child : Base {
	int _childVal;
}
- (instancetype)init;
- (int)childVal;
@end

@implementation Child
- (instancetype)init {
	self = [super init];
	_childVal = 20;
	return self;
}
- (int)childVal {
	return _childVal;
}
@end

#include <stdio.h>

int main(void) {
	Child *c = [Child alloc];
	c = [c init];
	printf(\"baseVal=%d\\n\", [c baseVal]);
	printf(\"childVal=%d\\n\", [c childVal]);
	[c release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "super_calls_parent_dispatch");
    assert_eq!(stdout, "baseVal=10\nchildVal=20\n");
}

/// The gap `method_override_dispatch` above leaves open: there, every
/// receiver's declared type already *is* its concrete class, so binding
/// the call to the declared type happens to be right. A declared type is
/// only an upper bound, though -- here `b` is declared `Base *` but holds
/// a `Sub`, so calling `Base`'s implementation directly would silently run
/// the wrong method instead of failing loudly.
///
/// The Python pipeline forces `{dealloc, init, isEqual:,
/// getDescription:maxLength:}` to protocol dispatch and otherwise
/// devirtualizes only when it can infer the receiver's *concrete* class
/// (`_try_infer_concrete_class`). oz_static decides by class hierarchy
/// analysis instead (`Program::has_overriding_subclass`): it sees the
/// whole program, so a direct call is kept exactly when no subclass
/// overrides the selector and routed through the class_id switch when one
/// does. `init` needs no special-casing under that rule -- an overridden
/// `init` is covered like any other selector, which the `tag` assertions
/// below check.
#[test]
fn override_through_base_typed_receiver_dispatches_dynamically() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Base : OZObject {
	int _tag;
}
- (instancetype)init;
- (int)speak;
- (int)tag;
@end
@implementation Base
- (instancetype)init {
	_tag = 10;
	return self;
}
- (int)speak {
	return 1;
}
- (int)tag {
	return _tag;
}
@end

@interface Sub : Base
- (instancetype)init;
- (int)speak;
@end
@implementation Sub
- (instancetype)init {
	[super init];
	_tag = 20;
	return self;
}
- (int)speak {
	return 2;
}
@end

#include <stdio.h>

int main(void) {
	/* Declared Base *, actually a Sub: the overrides must win. */
	Base *b = (Base *)[Sub alloc];
	[b init];
	printf(\"speak_through_base=%d\\n\", [b speak]);
	printf(\"tag_through_base=%d\\n\", [b tag]);
	[b release];

	/* A real Base is unaffected by the subclass's overrides. */
	Base *plain = [Base alloc];
	[plain init];
	printf(\"speak_plain=%d\\n\", [plain speak]);
	printf(\"tag_plain=%d\\n\", [plain tag]);
	[plain release];
	return 0;
}
"
    );
    let stdout =
        compile_and_run(&src, "override_through_base_typed_receiver_dispatches_dynamically");
    assert_eq!(
        stdout,
        "speak_through_base=2\ntag_through_base=20\nspeak_plain=1\ntag_plain=10\n"
    );
}

/// `-getDescription:maxLength:` gets a dispatcher even when nothing in the
/// program declares it as a protocol requirement.
///
/// This is the test that makes `model.rs`'s `ALWAYS_DYNAMIC` literal
/// load-bearing, and it did not exist before #413. That constant compares
/// a selector **as data**, which `docs/STATUS.md` singles out as the
/// rename hazard with no diagnostic -- and it fails *conditionally*, which
/// is why the rest of the suite could not see it: every other fixture
/// builds on the real `OZObject`, which adopts `OZObjectProtocol`, so
/// `is_protocol_selector` already answers yes and returns before
/// `ALWAYS_DYNAMIC` is consulted at all. Restoring the old spelling in
/// that list left all 568 other tests green.
///
/// So the root class here is hand-rolled and adopts nothing, and exactly
/// one class implements the selector -- otherwise the "more than one
/// implementor" arm would answer yes for its own reasons. Under those two
/// conditions `ALWAYS_DYNAMIC` is the only thing that can produce the
/// dispatcher, and a stale literal means `OZLog`'s `%@` has nothing to
/// call: `src/OZLog.c` names `OZ_PROTOCOL_SEND_getDescription_maxLength_`
/// directly, so the failure lands as a link error in a file that is
/// deliberately outside the pipeline.
#[test]
fn description_dispatches_dynamically_without_a_protocol_declaring_it() {
    let src = "\
#define nil ((id)0)
typedef bool BOOL;

__attribute__((objc_root_class))
@interface Root
+ (instancetype)alloc;
- (instancetype)init;
- (void)dealloc;
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen;
@end
@implementation Root
+ (instancetype)alloc {
	return nil;
}
- (instancetype)init {
	return self;
}
- (void)dealloc {
}
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen {
	(void)buf;
	(void)maxLen;
	return 0;
}
@end
";
    let out = oz_static::transpile(src).expect("hand-rolled root should transpile");
    let all = format!("{}{}{}", out.companion_h, out.companion_c, out.source_c);
    assert!(
        all.contains("OZ_PROTOCOL_SEND_getDescription_maxLength_"),
        "no dispatcher generated for -getDescription:maxLength: -- ALWAYS_DYNAMIC \
         in model.rs no longer names the selector this program spells:\n{}",
        all
    );
}
