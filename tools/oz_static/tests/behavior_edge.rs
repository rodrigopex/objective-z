// SPDX-License-Identifier: Apache-2.0
//
// behavior_edge.rs - port of the Python-pipeline "edge" behavior category
// (tests/behavior/cases/edge/) to oz_static, per OZ-092. All 8 upstream
// fixtures are in scope: multiple_args_method, nil_returns_zero,
// empty_class_no_methods, deep_inheritance, plus boxed_enum, boxed_float,
// boxed_expression, boxed_call_expr -- now that OZQ31 exists
// (behavior_foundation_q31.rs), the 4 boxed-literal fixtures are real
// accept+run tests against the real OZQ31 (via `common::OZQ31_SRC`), not
// reject tests -- @(expr) legitimately boxes an enum/float/arithmetic
// expression/function-call result in the real Python pipeline too.
//
// oz_static has no shared Foundation root yet, so every test declares its
// own `OZSRoot`, same as end_to_end_behavior.rs / static_bar_rejects.rs.
// This file's OZSRoot also declares `init` (returning self), since OZQ31's
// factory methods chain `[[OZQ31 alloc] init]`.

mod common;
use common::{compile_and_run, OZQ31_SRC};

const PREAMBLE: &str = "\
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

// --- boxed literals, via the real OZQ31 (OZ-092 Foundation work) -------

#[test]
fn boxed_enum_boxes_int_via_ozq31() {
    // boxed_enum.m: `_boxed = @(code);` where `code` is an enum-typed
    // (here: plain int, oz_static has no enum-in-param-position support
    // to spare) method parameter -- boxes through OZQ31's int32 path.
    let src = format!(
        "{}{}\n\
@interface BoxedEnumTest : OZSRoot {{
	struct OZQ31 *_boxed;
}}
- (void)boxStatus:(int)code;
- (int)boxedValue;
@end
@implementation BoxedEnumTest
- (void)boxStatus:(int)code {{
	_boxed = @(code);
}}
- (int)boxedValue {{
	return [_boxed int32Value];
}}
@end

#include <stdio.h>
int main(void) {{
	BoxedEnumTest *t = [BoxedEnumTest alloc];
	[t boxStatus:200];
	printf(\"ok=%d\\n\", [t boxedValue]);
	[t boxStatus:404];
	printf(\"not_found=%d\\n\", [t boxedValue]);
	return 0;
}}
",
        PREAMBLE, OZQ31_SRC
    );
    let stdout = compile_and_run(&src, "boxed_enum_boxes_int_via_ozq31");
    assert_eq!(stdout, "ok=200\nnot_found=404\n");
}

#[test]
fn boxed_float_boxes_float_var_via_ozq31() {
    // boxed_float.m: `_boxed = @(f);` where `f` is a float local -- the
    // boxed spelling (a bare identifier) carries no float hint, so this
    // exercises render_boxed_at_expression's scope-type fallback (see
    // emit.rs) rather than the literal-token heuristic.
    let src = format!(
        "{}{}\n\
@interface BoxedFloatTest : OZSRoot {{
	struct OZQ31 *_boxed;
}}
- (void)run;
- (float)boxedValue;
@end
@implementation BoxedFloatTest
- (void)run {{
	float f = 3.14f;
	_boxed = @(f);
}}
- (float)boxedValue {{
	return [_boxed floatValue];
}}
@end

#include <stdio.h>
int main(void) {{
	BoxedFloatTest *t = [BoxedFloatTest alloc];
	[t run];
	float v = [t boxedValue];
	printf(\"within_tolerance=%s\\n\", (v > 3.13f && v < 3.15f) ? \"ok\" : \"FAIL\");
	return 0;
}}
",
        PREAMBLE, OZQ31_SRC
    );
    let stdout = compile_and_run(&src, "boxed_float_boxes_float_var_via_ozq31");
    assert_eq!(stdout, "within_tolerance=ok\n");
}

#[test]
fn boxed_expression_boxes_var_expr_call_float_uint_via_ozq31() {
    // boxed_expression.m: boxes a bare variable, an arithmetic
    // expression, a function-call result, a float, and an unsigned int,
    // all via `@(...)`.
    let src = format!(
        "{}{}\n\
static int triple(int x) {{
	return x * 3;
}}

@interface BoxedTest : OZSRoot {{
	struct OZQ31 *_fromVar;
	struct OZQ31 *_fromExpr;
	struct OZQ31 *_fromCall;
	struct OZQ31 *_fromFloat;
	struct OZQ31 *_fromUint;
}}
- (void)run;
- (int)fromVarValue;
- (int)fromExprValue;
- (int)fromCallValue;
- (float)fromFloatValue;
- (int)fromUintValue;
@end

@implementation BoxedTest
- (void)run {{
	int val = 7;
	_fromVar = @(val);
	_fromExpr = @(val + 3);
	_fromCall = @(triple(val));
	float f = 2.5f;
	_fromFloat = @(f);
	unsigned int u = 1000;
	_fromUint = @(u);
}}
- (int)fromVarValue {{
	return [_fromVar int32Value];
}}
- (int)fromExprValue {{
	return [_fromExpr int32Value];
}}
- (int)fromCallValue {{
	return [_fromCall int32Value];
}}
- (float)fromFloatValue {{
	return [_fromFloat floatValue];
}}
- (int)fromUintValue {{
	return [_fromUint int32Value];
}}
@end

#include <stdio.h>
int main(void) {{
	BoxedTest *t = [BoxedTest alloc];
	[t run];
	printf(\"fromVar=%d\\n\", [t fromVarValue]);
	printf(\"fromExpr=%d\\n\", [t fromExprValue]);
	printf(\"fromCall=%d\\n\", [t fromCallValue]);
	float ff = [t fromFloatValue];
	printf(\"fromFloat=%s\\n\", (ff > 2.49f && ff < 2.51f) ? \"ok\" : \"FAIL\");
	printf(\"fromUint=%u\\n\", (unsigned int)[t fromUintValue]);
	return 0;
}}
",
        PREAMBLE, OZQ31_SRC
    );
    let stdout = compile_and_run(&src, "boxed_expression_boxes_var_expr_call_float_uint_via_ozq31");
    assert_eq!(
        stdout,
        "fromVar=7\nfromExpr=10\nfromCall=21\nfromFloat=ok\nfromUint=1000\n"
    );
}

#[test]
fn boxed_call_expr_boxes_function_result_via_ozq31() {
    // boxed_call_expr.m: `_boxed = @(computeValue());` -- a function
    // call's result boxed directly.
    let src = format!(
        "{}{}\n\
static int computeValue(void) {{
	return 99;
}}
@interface BoxedCallTest : OZSRoot {{
	struct OZQ31 *_boxed;
}}
- (void)run;
- (int)boxedValue;
@end
@implementation BoxedCallTest
- (void)run {{
	_boxed = @(computeValue());
}}
- (int)boxedValue {{
	return [_boxed int32Value];
}}
@end

#include <stdio.h>
int main(void) {{
	BoxedCallTest *t = [BoxedCallTest alloc];
	[t run];
	printf(\"boxed=%d\\n\", [t boxedValue]);
	return 0;
}}
",
        PREAMBLE, OZQ31_SRC
    );
    let stdout = compile_and_run(&src, "boxed_call_expr_boxes_function_result_via_ozq31");
    assert_eq!(stdout, "boxed=99\n");
}
