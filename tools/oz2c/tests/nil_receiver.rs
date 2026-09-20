// SPDX-License-Identifier: Apache-2.0
//
// nil_receiver.rs -- a message to nil is a no-op answering zero (#528).
//
// Objective-C's nil-receiver rule is the language's best-known safety
// property, and `[[Foo sharedInstance] bar]` depends on it. oz2c lowers a
// send to a direct call, so the receiver arrived as `self == NULL` and the
// body read `self->_field`.
//
// **It produced a wrong value before anyone was looking for it.** A second
// singleton's `+initialize` read a threshold through a receiver that
// happened to be nil and logged **20385** where the value was **80**; two
// deliberate sends printed 20413 and *drifted between builds*, exactly as
// the contents of address 0 would. On `mps2/an385` address 0 is flash, so
// it answers a plausible number rather than faulting -- which is why this
// was silent. On a part that traps a null read it would fault instead.
//
// Two guards, and both are needed. Enumerated rather than assumed, by
// walking every pointer-parameter dereference in a reflection-enabled
// companion: **9 of 12 were unguarded**, and every one was an
// `OZ_PROTOCOL_SEND_*`.
//
//   * **Each instance method tests its receiver on entry.** Covers a
//     direct call. In the callee rather than at the call site because it
//     covers every path to the method at once (direct, dispatcher,
//     `-performSelector:`, the dealloc chain), because one guard per
//     method is smaller than one per call site, and because a call-site
//     guard on `[[self make] poke]` would have to bind the receiver to a
//     temporary or evaluate it twice.
//   * **Each `OZ_PROTOCOL_SEND_*` tests it before switching.** The
//     dispatcher reads `self->_meta.class_id` to route, so it dereferences
//     the receiver *before* any guarded body is reached. The method
//     prologue cannot help there.
//
// A **class** method needs neither and costs nothing: it is emitted as
// `Foo_bar_cls(void)`, with no receiver parameter to be nil.
//
// Cost, measured across 17 configurations on `mps2/an385` by building the
// samples with and without: `.text` +832 bytes (+0.22%), `data` and `bss`
// unchanged. 65 guards are emitted for a minimal two-class program and
// only the reachable ones survive the linker.
//
// `CONFIG_OBJZ_NIL_SAFE_SENDS` (default `y`) is the switch, reaching oz2c
// as `--no-nil-safe-sends` -- the one *negative* feature flag in the tool,
// because the fail-safe direction is opposite to its siblings. Both
// configurations are exercised here: an option nothing tests rots.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// The body of the function whose signature is `signature`.
///
/// Anchored on `signature` followed by `\n{` deliberately: `source_c`
/// carries a **prototype** with the same text, so a plain `split` on the
/// signature lands after the declaration and the slice that follows is
/// some other function's. That cost two false failures here before it was
/// noticed -- the same shape as any extractor that guesses where a
/// function ends.
fn body_of<'a>(source_c: &'a str, signature: &str) -> &'a str {
    let start = source_c
        .find(&format!("{}\n{{", signature))
        .unwrap_or_else(|| panic!("no definition of `{}` in:\n{}", signature, source_c));
    let rest = &source_c[start..];
    let end = rest.find("\n}").map(|e| e + 2).unwrap_or(rest.len());
    &rest[..end]
}

fn unguarded_options() -> oz2c::Options {
    oz2c::Options { nil_sends_unchecked: true, ..Default::default() }
}

/// The issue's own shape, run rather than read: a scalar getter, an
/// object-returning method and a `void` setter, each sent to nil, with a
/// live object alongside to prove the guard did not break the ordinary
/// path.
#[test]
fn a_send_to_nil_answers_zero() {
    let src = format!(
        "{}
@interface NilProbe : OZObject {{
	int _threshold;
}}
- (int)threshold;
- (void)setThreshold:(int)v;
- (id)itself;
@end

@implementation NilProbe
- (int)threshold {{ return _threshold; }}
- (void)setThreshold:(int)v {{ _threshold = v; }}
- (id)itself {{ return self; }}
@end

#include <stdio.h>

int main(void) {{
	NilProbe *absent = nil;
	NilProbe *real = [NilProbe alloc];

	[real setThreshold:80];
	printf(\"real=%d\\n\", [real threshold]);

	/* The accidental case from the issue: a getter through a nil
	 * receiver, which logged 20385 where 80 was correct. */
	printf(\"nil_scalar=%d\\n\", [absent threshold]);
	printf(\"nil_object_is_nil=%d\\n\", [absent itself] == nil);

	/* A void send has nothing to answer with; it must simply not
	 * fault, and must not touch the live object either. */
	[absent setThreshold:5];
	printf(\"live_unchanged=%d\\n\", [real threshold]);
	return 0;
}}
",
        PREAMBLE()
    );
    let out = compile_and_run(&src, "nil_receiver_answers_zero");
    assert_eq!(
        out.trim().lines().collect::<Vec<_>>(),
        vec!["real=80", "nil_scalar=0", "nil_object_is_nil=1", "live_unchanged=80"]
    );
}

/// The guard is in the callee, so it is one guard per method however many
/// call sites there are. Asserted on the emitted text because that is the
/// claim the footprint number rests on.
#[test]
fn the_guard_is_per_method_not_per_call_site() {
    let src = format!(
        "{}
@interface Counted : OZObject
- (int)v;
@end
@implementation Counted
- (int)v {{ return 1; }}
@end

@interface Many : OZObject
- (int)run;
@end
@implementation Many
- (int)run
{{
	Counted *c = [Counted alloc];
	return [c v] + [c v] + [c v] + [c v];
}}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("transpiles");
    let body = body_of(&out.source_c, "int Counted_v(struct Counted *self)");
    assert_eq!(body.matches("if (!self)").count(), 1, "one guard in the callee:\n{}", body);
    /* And the four call sites carry none -- that is what makes the cost
     * proportional to methods rather than to sends. */
    let run = body_of(&out.source_c, "int Many_run(struct Many *self)");
    assert_eq!(run.matches("if (!self)").count(), 1, "only Many_run's own guard:\n{}", run);
    assert_eq!(run.matches("Counted_v(").count(), 4, "four unguarded call sites:\n{}", run);
}

/// A class method gets no guard, because `Foo_bar_cls(void)` has no
/// receiver to test. This is why the class side of a program costs
/// nothing, and it would regress silently into wasted bytes.
#[test]
fn a_class_method_gets_no_guard() {
    let src = format!(
        "{}
@interface Side : OZObject
+ (int)answer;
@end
@implementation Side
+ (int)answer {{ return 7; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("transpiles");
    let body = body_of(&out.source_c, "int Side_answer_cls(void)");
    assert!(!body.contains("if (!self)"), "a class method has no receiver:\n{}", body);
}

/// **The dispatcher's own guard**, which the method prologue cannot
/// provide: `OZ_PROTOCOL_SEND_*` reads `self->_meta.class_id` to route, so
/// it dereferences the receiver before any body is entered.
#[test]
fn a_protocol_dispatcher_guards_before_switching() {
    let src = format!(
        "{}
@protocol Pingable
- (int)ping;
@end

@interface P1 : OZObject <Pingable>
- (int)ping;
@end
@implementation P1
- (int)ping {{ return 1; }}
@end

@interface P2 : OZObject <Pingable>
- (int)ping;
@end
@implementation P2
- (int)ping {{ return 2; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("transpiles");
    let d = out
        .companion_c
        .split("OZ_PROTOCOL_SEND_ping(")
        .nth(1)
        .expect("two implementors should force a dispatcher");
    let guard = d.find("if (!self)");
    let switch = d.find("switch (self->_meta.class_id)");
    assert!(guard.is_some(), "the dispatcher needs its own guard:\n{}", &d[..400.min(d.len())]);
    assert!(
        guard < switch,
        "the guard must come *before* the class-id read, or it is useless:\n{}",
        &d[..400.min(d.len())]
    );
}

/// No pointer-parameter dereference anywhere in the companion is left
/// unguarded.
///
/// The standing check, and the one that found the dispatchers: keyed on
/// *dereferences* rather than on the parameter being called `self`, since
/// a receiver named `obj` would otherwise slip past and the sweep would
/// report a clean number for the wrong set.
#[test]
fn no_companion_function_dereferences_a_receiver_unguarded() {
    let src = format!(
        "{}
@protocol Pingable
- (int)ping;
@end
@interface Q1 : OZObject <Pingable>
- (int)ping;
@end
@implementation Q1
- (int)ping {{ return 1; }}
@end
@interface Q2 : OZObject <Pingable>
- (int)ping;
@end
@implementation Q2
- (int)ping {{ return 2; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("transpiles");

    let mut checked = 0;
    let mut unguarded: Vec<String> = Vec::new();
    for chunk in out.companion_c.split("\n\n") {
        let Some(open) = chunk.find("\n{") else {
            continue;
        };
        let (signature, body) = chunk.split_at(open);
        let Some(name) = signature.rsplit('\n').next() else {
            continue;
        };
        if !name.contains('(') || !body.contains("self->") {
            continue;
        }
        checked += 1;
        if !body.contains("if (!self)") && !body.contains("if (self &&") {
            unguarded.push(name.trim().to_string());
        }
    }
    assert!(checked > 0, "found no receiver-dereferencing companion function to check");
    assert!(unguarded.is_empty(), "unguarded receiver dereference(s): {:?}", unguarded);
}

/// A struct return zeroes through a compound literal, because `(T)0` is
/// not a conversion C allows to a struct type. `(T){0}` is C99 and
/// generated C is compiled as C17 with `-pedantic-errors`.
#[test]
fn a_struct_returning_send_to_nil_answers_a_zeroed_struct() {
    let src = format!(
        "{}
struct nr_range {{
	int lo;
	int hi;
}};

@interface Ranged : OZObject
- (struct nr_range)range;
@end
@implementation Ranged
- (struct nr_range)range
{{
	struct nr_range r = {{ 3, 9 }};
	return r;
}}
@end

#include <stdio.h>

int main(void) {{
	Ranged *absent = nil;
	Ranged *real = [Ranged alloc];
	struct nr_range a = [real range];
	struct nr_range b = [absent range];
	printf(\"real=%d,%d\\n\", a.lo, a.hi);
	printf(\"nil=%d,%d\\n\", b.lo, b.hi);
	return 0;
}}
",
        PREAMBLE()
    );
    let out = compile_and_run(&src, "nil_receiver_struct_return");
    assert_eq!(out.trim().lines().collect::<Vec<_>>(), vec!["real=3,9", "nil=0,0"]);
}

/// `--no-nil-safe-sends` (`CONFIG_OBJZ_NIL_SAFE_SENDS=n`) removes both
/// guards. Tested because an option nothing exercises rots, and because
/// this is the configuration a footprint-constrained user would pick --
/// they should get exactly the bytes back, not a half-guarded program.
#[test]
fn the_kconfig_option_removes_both_guards() {
    let src = format!(
        "{}
@protocol Pingable
- (int)ping;
@end
@interface R1 : OZObject <Pingable>
- (int)ping;
@end
@implementation R1
- (int)ping {{ return 1; }}
@end
@interface R2 : OZObject <Pingable>
- (int)ping;
@end
@implementation R2
- (int)ping {{ return 2; }}
@end
",
        PREAMBLE()
    );
    let guarded = oz2c::transpile(&src).expect("transpiles");
    let unguarded =
        oz2c::transpile_with_options(&src, &unguarded_options()).expect("transpiles");

    assert!(guarded.source_c.contains("if (!self)"), "the default is guarded");
    assert!(
        !unguarded.source_c.contains("if (!self)"),
        "--no-nil-safe-sends should leave no method guard"
    );

    /* Scoped to the dispatcher, **not** to `companion_c` as a whole:
     * `oz_class_name`, `oz_retain` and `oz_release` carry their own
     * nil guards in every build and predate this change, so asserting on
     * the whole file tests those instead of this. It failed that way
     * first. */
    let dispatcher = |c: &str| -> String {
        c.split("OZ_PROTOCOL_SEND_ping(")
            .nth(1)
            .map(|d| d[..d.find("\n}").unwrap_or(d.len())].to_string())
            .expect("two implementors should force a dispatcher")
    };
    assert!(
        dispatcher(&guarded.companion_c).contains("if (!self)"),
        "the default guards the dispatcher too"
    );
    assert!(
        !dispatcher(&unguarded.companion_c).contains("if (!self)"),
        "--no-nil-safe-sends should leave no dispatcher guard"
    );
    /* The unguarded output is still the old, smaller C -- so the flag buys
     * bytes back rather than merely renaming something. */
    assert!(
        unguarded.source_c.len() < guarded.source_c.len(),
        "the unchecked build should be smaller"
    );
}

/// The ordinary path is unaffected with the guards off, which is what
/// makes the option a footprint trade rather than a behaviour change for
/// code that never sends to nil.
#[test]
fn an_unguarded_build_still_runs_ordinary_sends() {
    let src = format!(
        "{}
@interface Plain : OZObject {{
	int _n;
}}
- (int)n;
- (void)setN:(int)v;
@end
@implementation Plain
- (int)n {{ return _n; }}
- (void)setN:(int)v {{ _n = v; }}
@end

#include <stdio.h>

int main(void) {{
	Plain *p = [Plain alloc];
	[p setN:41];
	printf(\"n=%d\\n\", [p n]);
	return 0;
}}
",
        PREAMBLE()
    );
    let out = common::compile_and_run_with_options(
        &src,
        "nil_receiver_unguarded_ordinary",
        &unguarded_options(),
    );
    assert_eq!(out.trim(), "n=41");
}
