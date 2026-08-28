// SPDX-License-Identifier: Apache-2.0
//
// behavior_dispatch.rs - OZ-092: port of the Python-pipeline "dispatch"
// behavior fixtures (tests/behavior/cases/dispatch/*.m + *_test.c) to the
// static-subset spike. Each Python fixture pair is a class declaration
// (.m) plus a hand-written Unity _test.c calling the generated API
// directly; here the class declarations and the assertions are folded
// into one source string with a `main()` that `printf`s the values under
// test, and the Rust test asserts the exact stdout -- same shape as
// end_to_end_behavior.rs. oz_static has no shared Foundation root yet, so
// every test declares its own synthetic `OZSRoot` (mirroring the other
// test files here), and `alloc`/`init`/inherited or overridden methods
// are exercised through ordinary `[receiver selector]` sends -- the
// static bar resolves the receiver's declared type and dispatches to the
// correct implementation at compile time, so there's no need to
// reproduce the oracle's raw `(struct Parent *)` casts by hand.

mod common;
use common::compile_and_run;

#[test]
fn class_method_dispatch() {
    // Ported from tests/behavior/cases/dispatch/class_method.m /
    // class_method_test.c: a class method (`+version`) dispatches and
    // returns its value, with no instance ever created.
    let src = "\
@interface OZSRoot
- (void)dealloc;
@end
@implementation OZSRoot
- (void)dealloc {
}
@end

@interface Factory : OZSRoot
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
";
    let stdout = compile_and_run(src, "class_method_dispatch");
    assert_eq!(stdout, "version=42\n");
}

#[test]
fn inherited_method_dispatch() {
    // Ported from inherited_method.m / inherited_method_test.c: Car
    // declares no methods of its own -- `[c speed]` must resolve up the
    // hierarchy to Vehicle's implementation.
    let src = "\
@interface OZSRoot
- (instancetype)init;
- (void)dealloc;
@end
@implementation OZSRoot
- (instancetype)init {
	return self;
}
- (void)dealloc {
}
@end

@interface Vehicle : OZSRoot {
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
";
    let stdout = compile_and_run(src, "inherited_method_dispatch");
    assert_eq!(stdout, "speed=60\n");
}

#[test]
fn method_override_dispatch() {
    // Ported from method_override.m / method_override_test.c: Dog
    // overrides Animal's `sound`; a Dog must call its own, and Animal
    // instances must be unaffected by the subclass's override.
    let src = "\
@interface OZSRoot
- (void)dealloc;
@end
@implementation OZSRoot
- (void)dealloc {
}
@end

@interface Animal : OZSRoot
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
";
    let stdout = compile_and_run(src, "method_override_dispatch");
    assert_eq!(stdout, "dog_sound=2\nanimal_sound=1\n");
}

#[test]
fn send_routes_correct_dispatch() {
    // Ported from send_routes_correct.m / send_routes_correct_test.c: a
    // plain instance method call routes to the correct implementation and
    // observably mutates the receiver's own ivar. `alloc` zero-initializes
    // storage, so `_spoken` starts at 0 with no explicit init needed.
    let src = "\
@interface OZSRoot
- (void)dealloc;
@end
@implementation OZSRoot
- (void)dealloc {
}
@end

@interface Speaker : OZSRoot {
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
";
    let stdout = compile_and_run(src, "send_routes_correct_dispatch");
    assert_eq!(stdout, "before=0\nafter=1\n");
}

#[test]
fn super_calls_parent_dispatch() {
    // Ported from super_calls_parent.m / super_calls_parent_test.c: a
    // three-level `[super init]` chain (Child -> Base -> OZSRoot), each
    // level setting its own ivar -- both must be observable afterward.
    let src = "\
@interface OZSRoot
- (instancetype)init;
- (void)dealloc;
@end
@implementation OZSRoot
- (instancetype)init {
	return self;
}
- (void)dealloc {
}
@end

@interface Base : OZSRoot {
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
";
    let stdout = compile_and_run(src, "super_calls_parent_dispatch");
    assert_eq!(stdout, "baseVal=10\nchildVal=20\n");
}
