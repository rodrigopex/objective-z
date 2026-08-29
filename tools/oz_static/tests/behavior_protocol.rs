// SPDX-License-Identifier: Apache-2.0
//
// behavior_protocol.rs - OZ-092 Phase 2: protocol conformance +
// protocol-typed dispatch, ported from tests/behavior/cases/protocol/.
//
// The Python oracle's companion _test.c files call a generated
// `OZ_PROTOCOL_SEND_{selector}` function directly, passing a receiver cast
// to the root class's pointer type -- i.e. "some object of unknown
// concrete class, known only to conform to a protocol declaring this
// selector." These tests port that same shape through real ObjC syntax
// instead: alloc a concrete class, assign it to a root-typed variable,
// and send the protocol-declared selector through that variable -- the
// static type oz_static sees at the call site is the root class, not the
// concrete one, so it must fall back to the same runtime dispatch the
// oracle's OZ_PROTOCOL_SEND_* exercises directly.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

#[test]
fn protocol_dispatch_routes_to_correct_class() {
    // Ported from switch_routes_correct.m: two classes conforming to the
    // same protocol, each with its own -toggle implementation.
    let src = format!(
        "{}\n\
@protocol Togglable
- (int)toggle;
@end

@interface LightSwitch : OZObject <Togglable> {{
	int _state;
}}
@end
@implementation LightSwitch
- (int)toggle {{ _state = !_state; return _state; }}
@end

@interface Fan : OZObject <Togglable> {{
	int _running;
}}
@end
@implementation Fan
- (int)toggle {{ _running = !_running; return _running + 10; }}
@end

#include <stdio.h>
int main(void) {{
	OZObject *ls = (OZObject *)[LightSwitch alloc];
	OZObject *f = (OZObject *)[Fan alloc];
	printf(\"light=%d fan=%d\\n\", [ls toggle], [f toggle]);
	[ls release];
	[f release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "protocol_dispatch_routes_to_correct_class");
    assert_eq!(stdout, "light=1 fan=11\n");
}

#[test]
fn protocol_inheritance_exposes_super_protocol_methods() {
    // Ported from protocol_inheritance.m: FastRunnable <Runnable> adds
    // -sprint on top of Runnable's -run; a class conforming to
    // FastRunnable must answer to both through the root-typed dispatch.
    let src = format!(
        "{}\n\
@protocol Runnable
- (int)run;
@end
@protocol FastRunnable <Runnable>
- (int)sprint;
@end

@interface Athlete : OZObject <FastRunnable>
@end
@implementation Athlete
- (int)run {{ return 5; }}
- (int)sprint {{ return 10; }}
@end

#include <stdio.h>
int main(void) {{
	OZObject *a = (OZObject *)[Athlete alloc];
	printf(\"run=%d sprint=%d\\n\", [a run], [a sprint]);
	[a release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "protocol_inheritance_exposes_super_protocol_methods");
    assert_eq!(stdout, "run=5 sprint=10\n");
}

#[test]
fn multiple_protocol_conformance() {
    // Ported from multiple_conformance.m: one class conforming to two
    // unrelated protocols, both dispatchable through the root type.
    let src = format!(
        "{}\n\
@protocol Readable
- (int)read;
@end
@protocol Writable
- (int)write;
@end

@interface Stream : OZObject <Readable, Writable>
@end
@implementation Stream
- (int)read {{ return 1; }}
- (int)write {{ return 2; }}
@end

#include <stdio.h>
int main(void) {{
	OZObject *s = (OZObject *)[Stream alloc];
	printf(\"read=%d write=%d\\n\", [s read], [s write]);
	[s release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "multiple_protocol_conformance");
    assert_eq!(stdout, "read=1 write=2\n");
}

#[test]
fn typed_protocol_var_dispatches_per_instance() {
    // Ported from typed_protocol_var.m: two unrelated classes both
    // conforming to Measurable, each reached through the same root-typed
    // variable pattern -- proves dispatch is resolved per concrete
    // instance, not baked in at the call site.
    let src = format!(
        "{}\n\
@protocol Measurable
- (int)measure;
@end

@interface Ruler : OZObject <Measurable>
@end
@implementation Ruler
- (int)measure {{ return 30; }}
@end

@interface Scale : OZObject <Measurable>
@end
@implementation Scale
- (int)measure {{ return 100; }}
@end

#include <stdio.h>
int main(void) {{
	OZObject *r = (OZObject *)[Ruler alloc];
	OZObject *sc = (OZObject *)[Scale alloc];
	printf(\"ruler=%d scale=%d\\n\", [r measure], [sc measure]);
	[r release];
	[sc release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "typed_protocol_var_dispatches_per_instance");
    assert_eq!(stdout, "ruler=30 scale=100\n");
}
