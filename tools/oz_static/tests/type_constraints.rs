// SPDX-License-Identifier: Apache-2.0
//
// type_constraints.rs - `id<Protocol>` and `Container<Arg, ...>`
// constraint checking (see src/generics.rs).
//
// The oracle's own coverage for this (test_resolve.py, OZ-057, in the retired
// Python pipeline) was hand-built Clang-AST-node fixtures, not real `.m` source --
// its own comment says so ("Corner case: these tests use handcrafted AST
// nodes with generics preserved... real Clang-based generic validation is
// covered by source-level [golden] tests"). There is no real `.m` file to
// port verbatim, so the cases below are written directly against the
// scenarios that suite covers (protocol constraint match/mismatch, array
// class constraint match/mismatch, subclass satisfying a constraint,
// assignment -- not just initialization -- to an already-declared
// constrained var), using `common::expect_reject`/`compile_and_run` like
// every other test in this crate.
//
// CST shapes referenced below were confirmed empirically via a throwaway
// probe against `oz_static::parse::parse` before writing src/generics.rs
// (see that file's header comment for the design rationale); not
// re-verified here.

mod common;
use common::{compile_and_run, expect_reject, ozarray_src, ozdictionary_src, ozobject_src as PREAMBLE, ozstring_src};

/// `id<Proto> x = [ClassName ...];` where `ClassName` doesn't conform.
#[test]
fn id_protocol_mismatch_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Frobbable
- (void)frob;
@end

@interface Sensor : OZObject
- (void)read;
@end
@implementation Sensor
- (void)read {}
@end

@implementation User
- (void)test {
	id<Frobbable> x = [Sensor alloc];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("generic type mismatch"), "diagnostics: {}", diags);
    assert!(diags.contains("'Sensor'"), "diagnostics: {}", diags);
    assert!(diags.contains("'id<Frobbable>'"), "diagnostics: {}", diags);
}

/// The accepted counterpart: `ClassName` conforms directly.
#[test]
fn id_protocol_match_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Frobbable
- (void)frob;
@end

@interface Widget : OZObject <Frobbable>
- (void)frob;
@end
@implementation Widget
- (void)frob {}
@end

@interface User : OZObject
- (void)test;
@end
@implementation User
- (void)test {
	id<Frobbable> x = [Widget alloc];
	[x frob];
}
@end

#include <stdio.h>
int main(void) {
	User *u = [User alloc];
	[u test];
	printf(\"ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "id_protocol_match_accepted");
    assert_eq!(stdout, "ok\n");
}

/// Conformance inherited through a superclass must also satisfy the
/// constraint -- `Program::class_conforms_to` walks the superclass
/// chain, not just a class's own declared list.
#[test]
fn id_protocol_match_via_superclass_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Frobbable
- (void)frob;
@end

@interface Widget : OZObject <Frobbable>
- (void)frob;
@end
@implementation Widget
- (void)frob {}
@end

@interface SpecialWidget : Widget
@end
@implementation SpecialWidget
@end

@interface User : OZObject
- (void)test;
@end
@implementation User
- (void)test {
	id<Frobbable> x = [SpecialWidget alloc];
	[x frob];
}
@end

#include <stdio.h>
int main(void) {
	User *u = [User alloc];
	[u test];
	printf(\"ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "id_protocol_match_via_superclass_accepted");
    assert_eq!(stdout, "ok\n");
}

/// A plain assignment (not just the initializer) to an already-declared
/// `id<Proto>` slot must be checked too.
#[test]
fn id_protocol_mismatch_on_later_assignment_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Frobbable
- (void)frob;
@end

@interface Sensor : OZObject
@end
@implementation Sensor
@end

@implementation User
- (void)test {
	id<Frobbable> x = 0;
	x = [Sensor alloc];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("generic type mismatch"), "diagnostics: {}", diags);
}

/// `Container<Arg> *v = @[...];` -- each element checked against `Arg`.
/// Uses the real `OZArray` fixture (`common::ozarray_src`), not a hand
/// stub -- the boxed-array-literal desugar depends on `OZArray`'s actual
/// ivar shape (`companion::render_array_support`), so a stub class named
/// `OZArray` with no matching ivars fails to compile downstream of this
/// pass entirely, independent of anything this test is trying to check.
#[test]
fn array_generic_element_class_mismatch_rejected() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        "\
@interface Widget : OZObject
@end
@implementation Widget
@end

@interface Sensor : OZObject
@end
@implementation Sensor
@end

@implementation User
- (void)test {
	OZArray<Widget *> *a = @[ [Widget alloc], [Sensor alloc] ];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("generic type mismatch"), "diagnostics: {}", diags);
    assert!(diags.contains("'Sensor'"), "diagnostics: {}", diags);
    assert!(diags.contains("'OZArray<Widget *>'"), "diagnostics: {}", diags);
}

/// A subclass of the element constraint satisfies it -- covariance, not
/// exact-type matching.
#[test]
fn array_generic_element_subclass_satisfies_constraint() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        "\
@interface Widget : OZObject
@end
@implementation Widget
@end

@interface SpecialWidget : Widget
@end
@implementation SpecialWidget
@end

@interface User : OZObject
- (void)test;
@end
@implementation User
- (void)test {
	OZArray<Widget *> *a = @[ [Widget alloc], [SpecialWidget alloc] ];
}
@end

#include <stdio.h>
int main(void) {
	User *u = [User alloc];
	[u test];
	printf(\"ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "array_generic_element_subclass_satisfies_constraint");
    assert_eq!(stdout, "ok\n");
}

/// `OZDictionary<K, V>` checks keys and values independently -- a
/// mismatched value with a matching key must still be caught. Uses the
/// real `OZDictionary`/`OZString` fixtures for the same reason
/// `array_generic_element_class_mismatch_rejected` does.
#[test]
fn dictionary_generic_value_mismatch_rejected() {
    let src = format!(
        "{}{}{}{}",
        PREAMBLE(),
        ozstring_src(),
        ozdictionary_src(),
        "\
@interface Widget : OZObject
@end
@implementation Widget
@end

@interface Sensor : OZObject
@end
@implementation Sensor
@end

@implementation User
- (void)test {
	OZDictionary<OZString *, Widget *> *d = @{ [OZString alloc] : [Sensor alloc] };
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("generic type mismatch"), "diagnostics: {}", diags);
    assert!(diags.contains("value 'Sensor'"), "diagnostics: {}", diags);
}

/// A value whose concrete class this pass cannot resolve (here, an
/// element that is itself a plain `id`-typed parameter forwarded
/// through) is left unchecked rather than misreported -- silence on the
/// unknown, matching the oracle's own `elem_type == "id": continue`.
#[test]
fn unresolvable_element_type_not_flagged() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        "\
@interface Widget : OZObject
@end
@implementation Widget
@end

@interface User : OZObject
- (void)test:(id)anything;
@end
@implementation User
- (void)test:(id)anything {
	OZArray<Widget *> *a = @[ anything ];
}
@end

#include <stdio.h>
int main(void) {
	User *u = [User alloc];
	Widget *w = [Widget alloc];
	[u test:w];
	printf(\"ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "unresolvable_element_type_not_flagged");
    assert_eq!(stdout, "ok\n");
}

/// A plain, non-generic collection type (`OZArray *`, no `<...>`) is not
/// itself a constraint -- this is the regression this feature's own
/// rollout tripped over (see the fix to `generics::classify_declared_type`):
/// every existing for-in behavior test declares locals this way, and
/// must keep compiling unconstrained.
#[test]
fn plain_class_type_is_not_a_constraint() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        "\
@interface Widget : OZObject
@end
@implementation Widget
@end

@interface User : OZObject
- (void)test;
@end
@implementation User
- (void)test {
	OZArray *a = [OZArray alloc];
	Widget *w = [Widget alloc];
}
@end

#include <stdio.h>
int main(void) {
	User *u = [User alloc];
	[u test];
	printf(\"ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "plain_class_type_is_not_a_constraint");
    assert_eq!(stdout, "ok\n");
}

/// A class declaring both its own generic parameter list *and* protocol
/// conformance (`@interface Box<__covariant T> : OZObject <Frobbable>`)
/// has two `parameterized_arguments` CST nodes -- see
/// `collect::extract_conformance`'s doc comment. Before that fix, the
/// conformance extractor always took the *first* one (the generic
/// parameter list), so it would read `__covariant`/`T` as if they were
/// protocol names -- meaning it would never find the *real* conformance
/// list, and this class's missing `-frob` implementation would go
/// undetected instead of being rejected. This is exactly the shape
/// `ozarray_src()`/`ozdictionary_src()` exercise for real, now that
/// their generic parameter is no longer cut from the fixture (see
/// `common::ozarray_src`'s doc comment) -- this test isolates just the
/// conformance-list mix-up on a minimal class, independent of anything
/// else `OZArray`/`OZDictionary` bring in.
#[test]
fn conformance_check_not_confused_by_generic_param_list() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Frobbable
- (void)frob;
@end

@interface Box<__covariant T> : OZObject <Frobbable> {
	int _n;
}
@end
@implementation Box
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("declares conformance to 'Frobbable'"), "diagnostics: {}", diags);
    assert!(diags.contains("doesn't implement 'frob'"), "diagnostics: {}", diags);
}

/// The accepted counterpart: the same shape, but `-frob` is actually
/// implemented.
#[test]
fn conformance_check_still_passes_with_generic_param_list() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Frobbable
- (void)frob;
@end

@interface Box<__covariant T> : OZObject <Frobbable> {
	int _n;
}
- (void)frob;
@end
@implementation Box
- (void)frob {
	_n = 1;
}
@end

#include <stdio.h>
int main(void) {
	Box *b = [Box alloc];
	[b frob];
	printf(\"ok\\n\");
	[b release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "conformance_check_still_passes_with_generic_param_list");
    assert_eq!(stdout, "ok\n");
}
