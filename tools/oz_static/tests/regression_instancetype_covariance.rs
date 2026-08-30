// SPDX-License-Identifier: Apache-2.0
//
// regression_instancetype_covariance.rs - OZ-100 regression tests.
//
// `instancetype` covaries with the *receiver's* static type, not with
// whichever class actually defines/implements the method -- three spots
// in emit.rs/companion.rs got this wrong, surfaced by piloting a real
// `#import <Foundation/Foundation.h>` build through oz_static (not just
// oz_static's own hand-built fixtures):
//
//   1. `[[Sub alloc] init]` where `Sub` inherits `-init` from an ancestor
//      (the plain instance-message path in emit.rs's render_message).
//   2. `self = [super init]` inside a subclass's own override (the
//      `super`-receiver path, which reports the *superclass*'s pointer
//      type as recv_type and so masked the same check case 1 uses).
//   3. A dynamically-dispatched (protocol) selector returning
//      `instancetype`, implemented by two+ classes each returning their
//      own struct pointer -- the shared `OZ_PROTOCOL_SEND_*` dispatch
//      function can only have one C return type.
//
// Plain `cc` (Apple clang, this host) only *warns* on a wrong struct
// pointer type by default -- unlike the real embedded GCC toolchain
// (Zephyr's arm-zephyr-eabi-gcc 14), which errors on it -- so these use
// `compile_and_run_strict` (`-Werror=incompatible-pointer-types`) to
// actually fail if any of the three regress, the way `compile_and_run`
// alone would not have.

mod common;
use common::{compile_and_run_strict, ozobject_src as PREAMBLE};

#[test]
fn inherited_init_covaries_with_subclass_receiver() {
    // Case 1: Sub declares no -init of its own -- alloc/init chains
    // through OZObject's, whose `instancetype` must resolve to `Sub *`
    // at this call site, not `OZObject *`.
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Sub : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end
@implementation Sub
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
@end

#include <stdio.h>

int main(void) {
	Sub *s = [[Sub alloc] init];
	[s setTag:42];
	printf(\"tag=%d\\n\", [s tag]);
	[s release];
	return 0;
}
"
    );
    let stdout = compile_and_run_strict(&src, "inherited_init_covaries_with_subclass_receiver");
    assert_eq!(stdout, "tag=42\n");
}

#[test]
fn super_init_covaries_with_enclosing_class() {
    // Case 2: Vehicle overrides -init and chains via `self = [super
    // init]` -- the assignment target (`self`, typed `struct Vehicle
    // *`) must accept the cast-back result, not OZObject's own type.
    let src = format!(
        "{}{}",
        PREAMBLE(),
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
- (int)speed { return _speed; }
@end

#include <stdio.h>

int main(void) {
	Vehicle *v = [Vehicle alloc];
	v = [v init];
	printf(\"speed=%d\\n\", [v speed]);
	[v release];
	return 0;
}
"
    );
    let stdout = compile_and_run_strict(&src, "super_init_covaries_with_enclosing_class");
    assert_eq!(stdout, "speed=60\n");
}

#[test]
fn protocol_dispatch_instancetype_covaries_per_implementor() {
    // Case 3: two classes conforming to `Cloneable`, each returning
    // *its own* concrete type from `-clone`. Both `-clone` and the
    // `-fingerprint` check are sent through root-typed variables (`wg`,
    // `gg`, `wc`, `gc`) so they route through the shared dynamic
    // `OZ_PROTOCOL_SEND_*` companion functions -- calling through a
    // concretely-typed variable would resolve statically instead and
    // never exercise the shared dispatch function this bug was in.
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Cloneable
- (instancetype)clone;
- (int)fingerprint;
@end

@interface Widget : OZObject <Cloneable> {
	int _id;
}
- (void)setId:(int)i;
@end
@implementation Widget
- (void)setId:(int)i { _id = i; }
- (int)fingerprint { return _id; }
- (instancetype)clone {
	Widget *w = [Widget alloc];
	[w setId:_id];
	return w;
}
@end

@interface Gadget : OZObject <Cloneable> {
	int _serial;
}
- (void)setSerial:(int)s;
@end
@implementation Gadget
- (void)setSerial:(int)s { _serial = s; }
- (int)fingerprint { return _serial; }
- (instancetype)clone {
	Gadget *g = [Gadget alloc];
	[g setSerial:_serial];
	return g;
}
@end

#include <stdio.h>

int main(void) {
	Widget *w = [Widget alloc];
	[w setId:7];
	OZObject *wg = (OZObject *)w;
	OZObject *wc = [wg clone];
	printf(\"widget_clone=%d\\n\", [wc fingerprint]);
	[w release];
	[wc release];

	Gadget *g = [Gadget alloc];
	[g setSerial:9];
	OZObject *gg = (OZObject *)g;
	OZObject *gc = [gg clone];
	printf(\"gadget_clone=%d\\n\", [gc fingerprint]);
	[g release];
	[gc release];
	return 0;
}
"
    );
    let stdout =
        compile_and_run_strict(&src, "protocol_dispatch_instancetype_covaries_per_implementor");
    assert_eq!(stdout, "widget_clone=7\ngadget_clone=9\n");
}
