// SPDX-License-Identifier: Apache-2.0
//
// unused_param_acks.rs -- `(void)param;` for parameters a translated method
// body never mentions (#229).
//
// `-Wunused-parameter` is `-Wextra` only, so these were never build failures --
// Zephyr's default warning set does not include it. They mattered because they
// were noise: three of the four defects gap M found were visible only because
// someone compiled the samples with `-Wall -Wextra` and counted warnings by
// kind, and 58 unused-parameter lines made that harder than it should be.
//
// The acknowledgement is the same one the SDK's own C already uses --
// `(void)inner;` in `oz_platform.h`'s heap stubs, `(void)expr;` in
// `oz_sdk/assert.h`.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

fn body_of<'a>(source_c: &'a str, signature: &str) -> &'a str {
    let start = source_c
        .find(&format!("{}\n{{", signature))
        .unwrap_or_else(|| panic!("no definition of `{}` in:\n{}", signature, source_c));
    let rest = &source_c[start..];
    let end = rest.find("\n}").map(|e| e + 2).unwrap_or(rest.len());
    &rest[..end]
}

const DECLS: &str = "\
@interface Foo : OZObject {
	int _n;
}
- (void)dealloc;
- (int)useSome:(int)a other:(int)b;
- (int)useAll:(int)a other:(int)b;
- (int)viaIvarOnly;
+ (int)classMethod:(int)z;
@end
@implementation Foo
- (void)dealloc { }
- (int)useSome:(int)a other:(int)b { return a; }
- (int)useAll:(int)a other:(int)b { return a + b + _n; }
- (int)viaIvarOnly { return _n; }
+ (int)classMethod:(int)z { return 1; }
@end
int main(void) { return 0; }
";

fn transpiled() -> String {
    let src = format!("{}{}", PREAMBLE(), DECLS);
    oz_static::transpile(&src).expect("should transpile").source_c
}

/// An empty `-dealloc` is idiomatic Objective-C, so the warning fired on
/// entirely correct code. This is the bulk of the 58.
#[test]
fn empty_dealloc_acknowledges_self() {
    let out = transpiled();
    let body = body_of(&out, "void Foo_dealloc(struct Foo *self)");
    assert!(body.contains("(void)self;"), "expected an ack for self; got:\n{}", body);
}

/// Only the parameters the body does not mention. `a` is used, `b` is not.
#[test]
fn only_unmentioned_parameters_are_acknowledged() {
    let out = transpiled();
    let body = body_of(&out, "int Foo_useSome_other_(struct Foo *self, int a, int b)");
    assert!(body.contains("(void)b;"), "expected an ack for b; got:\n{}", body);
    assert!(!body.contains("(void)a;"), "`a` is used; must not be acked:\n{}", body);
    assert!(body.contains("(void)self;"), "self is unused here; got:\n{}", body);
}

/// Nothing at all when every parameter is used.
#[test]
fn fully_used_signature_gets_no_acknowledgements() {
    let out = transpiled();
    let body = body_of(&out, "int Foo_useAll_other_(struct Foo *self, int a, int b)");
    assert!(!body.contains("(void)"), "no acks expected; got:\n{}", body);
}

/// The case that caught a flaw in the first implementation: an ivar reference
/// lowers to `self->_n`, so a body whose *source* never writes `self` still
/// uses the parameter. Deciding from the Objective-C source emitted a
/// redundant `(void)self;` here; deciding from the rendered C does not.
#[test]
fn ivar_reference_counts_as_using_self() {
    let out = transpiled();
    let body = body_of(&out, "int Foo_viaIvarOnly(struct Foo *self)");
    assert!(
        body.contains("self->_n"),
        "expected the ivar to lower through self; got:\n{}",
        body
    );
    assert!(
        !body.contains("(void)self;"),
        "`self` is used via the ivar lowering, so it must not be acked:\n{}",
        body
    );
}

/// A class method has no `self` to acknowledge, and its own unused parameter
/// still gets one.
#[test]
fn class_method_has_no_self_but_still_acks_its_parameters() {
    let out = transpiled();
    let body = body_of(&out, "int Foo_classMethod__cls(int z)");
    assert!(body.contains("(void)z;"), "expected an ack for z; got:\n{}", body);
    assert!(!body.contains("(void)self;"), "a class method has no self:\n{}", body);
}

/// Word-boundary matched, so a longer name containing a parameter's spelling
/// does not count as a use of it. `_next` must not make `n` look used.
#[test]
fn a_longer_name_containing_the_parameter_is_not_a_use() {
    let src = format!(
        "{}\
@interface Bar : OZObject {{
	int _next;
}}
- (int)take:(int)n;
@end
@implementation Bar
- (int)take:(int)n {{ return _next; }}
@end
int main(void) {{ return 0; }}
",
        PREAMBLE()
    );
    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    let body = body_of(&out, "int Bar_take_(struct Bar *self, int n)");
    assert!(
        body.contains("(void)n;"),
        "`n` is unused -- `_next` must not count as a use; got:\n{}",
        body
    );
}

/// The acknowledgements must not change what the code does. Runs a method
/// whose signature is partly unused and checks the value still comes back.
#[test]
fn acknowledgements_do_not_change_behaviour() {
    let src = format!(
        "{}\
@interface Calc : OZObject
- (int)pick:(int)a ignore:(int)b;
@end
@implementation Calc
- (int)pick:(int)a ignore:(int)b {{ return a * 2; }}
@end

#include <stdio.h>
int main(void) {{
	Calc *c = [Calc alloc];
	printf(\"v=%d\\n\", [c pick:21 ignore:99]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "acknowledgements_do_not_change_behaviour");
    assert_eq!(stdout, "v=42\n");
}
