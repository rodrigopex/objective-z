// SPDX-License-Identifier: Apache-2.0
//
// behavior_edge.rs - port of the Python-pipeline "edge" behavior category
// (tests/behavior/cases/edge/) to oz_static, per OZ-092. Only 4 of the 8
// upstream fixtures are in scope here (multiple_args_method,
// nil_returns_zero, empty_class_no_methods, deep_inheritance); the other
// 4 (boxed_enum, boxed_float, boxed_expression, boxed_call_expr) all use
// ObjC `@`-boxed-literal syntax, which the static subset hard-rejects by
// design (boxed-literal support is tracked separately as issue #190
// Phase 2) -- those are covered below as `expect_reject` tests instead of
// ports.
//
// oz_static has no shared Foundation root yet, so every test declares its
// own `OZSRoot` (dealloc as a no-op is enough), same as
// end_to_end_behavior.rs / static_bar_rejects.rs.

mod common;
use common::{compile_and_run, expect_reject};

const PREAMBLE: &str = "\
@interface OZSRoot
- (void)dealloc;
@end
@implementation OZSRoot
- (void)dealloc {
}
@end
";

#[test]
fn multiple_args_method() {
    // Ported from tests/behavior/cases/edge/multiple_args_method.m /
    // _test.c: a 3-argument instance method (`Calc_addA_b_c_`), checked
    // against both a normal sum and a sum that nets to zero.
    let src = format!(
        "{}\n\
@interface Calc : OZSRoot
- (int)addA:(int)a b:(int)b c:(int)c;
@end
@implementation Calc
- (int)addA:(int)a b:(int)b c:(int)c {{
	return a + b + c;
}}
@end

#include <stdio.h>

int main(void) {{
	Calc *m = [Calc alloc];
	printf(\"sum=%d\\n\", [m addA:10 b:20 c:30]);
	printf(\"sum2=%d\\n\", [m addA:-5 b:3 c:2]);
	[m release];
	return 0;
}}
",
        PREAMBLE
    );
    let stdout = compile_and_run(&src, "multiple_args_method");
    assert_eq!(stdout, "sum=60\nsum2=0\n");
}

#[test]
fn nil_returns_zero() {
    // Ported from tests/behavior/cases/edge/nil_returns_zero.m / _test.c.
    // The Python pipeline's version also checks
    // `OZObject_retainCount((struct OZObject *)0) == 0`; oz_static has no
    // retainCount equivalent at all (retain/release are the only
    // refcount operations the static subset models -- there is no
    // generated per-class or root `_retainCount` C API to call), so that
    // third assertion is skipped as genuinely out of scope rather than
    // faked. The other two (retaining nil is a no-op that yields nil;
    // releasing nil never dereferences it) port directly onto
    // oz_static_retain/oz_static_release.
    let src = format!(
        "{}\n\
#include <stdio.h>

int main(void) {{
	OZSRoot *r = 0;
	OZSRoot *result = [r retain];
	printf(\"retain_nil_is_null=%d\\n\", result == 0);
	[r release];
	printf(\"release_nil_ok\\n\");
	return 0;
}}
",
        PREAMBLE
    );
    let stdout = compile_and_run(&src, "nil_returns_zero");
    assert_eq!(stdout, "retain_nil_is_null=1\nrelease_nil_ok\n");
}

#[test]
fn empty_class_no_methods() {
    // Ported from tests/behavior/cases/edge/empty_class_no_methods.m /
    // _test.c: a class with no ivars and no declared methods must still
    // get a working alloc, a correctly-assigned class id, and a refcount
    // that starts at 1. oz_static has no `__objc_refcount_get` helper, so
    // the refcount is read directly via the PAL's `oz_atomic_get` on the
    // root-synthesized `oz_refcount` field, reached through the `base`
    // hop since EmptyClass isn't itself the root.
    let src = format!(
        "{}\n\
@interface EmptyClass : OZSRoot
@end
@implementation EmptyClass
@end

#include <stdio.h>

int main(void) {{
	EmptyClass *obj = [EmptyClass alloc];
	printf(\"nonnull=%d\\n\", obj != 0);
	printf(\"class_id=%d\\n\", obj->base.oz_class_id == OZ_STATIC_CLASS_EmptyClass);
	printf(\"refcount=%d\\n\", oz_atomic_get(&obj->base.oz_refcount));
	[obj release];
	return 0;
}}
",
        PREAMBLE
    );
    let stdout = compile_and_run(&src, "empty_class_no_methods");
    assert_eq!(stdout, "nonnull=1\nclass_id=1\nrefcount=1\n");
}

#[test]
fn deep_inheritance() {
    // Ported from tests/behavior/cases/edge/deep_inheritance.m / _test.c:
    // a 4-level inheritance chain (Level1..Level4) where every level
    // overrides `-depth` with its own literal return value and no level
    // calls super -- so each `Level*_depth` call must dispatch to that
    // exact class's own override, not an ancestor's.
    let src = format!(
        "{}\n\
@interface Level1 : OZSRoot
- (int)depth;
@end
@implementation Level1
- (int)depth {{
	return 1;
}}
@end

@interface Level2 : Level1
- (int)depth;
@end
@implementation Level2
- (int)depth {{
	return 2;
}}
@end

@interface Level3 : Level2
- (int)depth;
@end
@implementation Level3
- (int)depth {{
	return 3;
}}
@end

@interface Level4 : Level3
- (int)depth;
@end
@implementation Level4
- (int)depth {{
	return 4;
}}
@end

#include <stdio.h>

int main(void) {{
	Level4 *l4 = [Level4 alloc];
	Level3 *l3 = [Level3 alloc];
	Level1 *l1 = [Level1 alloc];
	printf(\"level4=%d\\n\", [l4 depth]);
	printf(\"level3=%d\\n\", [l3 depth]);
	printf(\"level1=%d\\n\", [l1 depth]);
	[l4 release];
	[l3 release];
	[l1 release];
	return 0;
}}
",
        PREAMBLE
    );
    let stdout = compile_and_run(&src, "deep_inheritance");
    assert_eq!(stdout, "level4=4\nlevel3=3\nlevel1=1\n");
}

// --- out of scope: boxed literals (issue #190 Phase 2) -----------------
//
// tests/behavior/cases/edge/boxed_enum.m, boxed_float.m, boxed_expression.m
// and boxed_call_expr.m all exercise ObjC `@`-boxed-literal syntax
// (`@(code)`, `@(f)`, `@(val + 3)`, `@(computeValue())`) to build an
// OZQ31 via the Python pipeline's boxing machinery. The static subset has
// no boxed-literal support and none is being added here -- instead each
// fixture is represented by a reject test confirming the static bar
// actually names and rejects the construct (never silently passes it
// through as broken C).

#[test]
fn boxed_enum_rejected() {
    // boxed_enum.m: `_boxed = @(code);` where `code` is an enum-typed
    // method parameter.
    let src = format!(
        "{}\n\
@interface BoxedEnumTest : OZSRoot {{
	int _boxed;
}}
- (void)boxStatus:(int)code;
@end
@implementation BoxedEnumTest
- (void)boxStatus:(int)code {{
	_boxed = @(code);
}}
@end
",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("boxed_expression"), "diagnostics: {}", diags);
}

#[test]
fn boxed_float_rejected() {
    // boxed_float.m: `_boxed = @(f);` where `f` is a float local.
    let src = format!(
        "{}\n\
@interface BoxedFloatTest : OZSRoot {{
	int _boxed;
}}
- (void)run;
@end
@implementation BoxedFloatTest
- (void)run {{
	float f = 3.14f;
	_boxed = @(f);
}}
@end
",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("boxed_expression"), "diagnostics: {}", diags);
}

#[test]
fn boxed_expression_rejected() {
    // boxed_expression.m: `_fromExpr = @(val + 3);` -- an arithmetic
    // expression boxed directly.
    let src = format!(
        "{}\n\
@interface BoxedTest : OZSRoot {{
	int _fromExpr;
}}
- (void)run;
@end
@implementation BoxedTest
- (void)run {{
	int val = 7;
	_fromExpr = @(val + 3);
}}
@end
",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("boxed_expression"), "diagnostics: {}", diags);
}

#[test]
fn boxed_call_expr_rejected() {
    // boxed_call_expr.m: `_boxed = @(computeValue());` -- a function
    // call's result boxed directly.
    let src = format!(
        "{}\n\
static int computeValue(void) {{
	return 99;
}}
@interface BoxedCallTest : OZSRoot {{
	int _boxed;
}}
- (void)run;
@end
@implementation BoxedCallTest
- (void)run {{
	_boxed = @(computeValue());
}}
@end
",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("boxed_expression"), "diagnostics: {}", diags);
}
