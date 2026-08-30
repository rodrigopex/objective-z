// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_q31.rs - OZ-092 Phase 2 (reordered ahead of for-in):
// OZQ31, the fixed-point number Foundation class, ported from
// tests/behavior/cases/foundation/{number_basic,q31_basic,q31_stdio_free}.m.
//
// Uses the real `OZObject` (`common::ozobject_src`) as the root class.
// OZQ31 itself is transplanted verbatim from the real `src/OZQ31.m` /
// `include/oz_sdk/Foundation/OZQ31.h` -- same helper function bodies
// (`_oz_bits_for_mag`, `_oz_shift_for_*`, `_oz_encode_*`, `_oz_decode_*`,
// `_oz_align_shift`, `_oz_q31_to_str`, `_oz_q31_div`), same method
// bodies, including `other->_raw` cross-instance ivar access and
// `[[OZQ31 alloc] init]` chaining -- both confirmed to transpile and run
// correctly through oz_static before this file was written. This is the
// real oracle algorithm, not a re-derivation, so parity with the Python
// pipeline's actual behavior is exact by construction rather than by
// matching documented comments.
//
// A boxed literal (`@42`, `@(expr)`, `@3.5f`) desugars to a class-method
// call on a class literally named `OZQ31` (see
// `emit::render_boxed_at_expression`) -- so none of this needed new
// transpiler features beyond that desugaring; it's ordinary ObjC/C source.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE, ozq31_src};

#[test]
fn number_basic_boxes_int_literal() {
    // number_basic.m
    let src = format!(
        "{}{}\n\
@interface NumTest : OZObject
- (int)boxed;
@end
@implementation NumTest
- (int)boxed {{
	OZQ31 *n = @(42);
	int v = [n intValue];
	return v;
}}
@end

#include <stdio.h>
int main(void) {{
	NumTest *t = [NumTest alloc];
	printf(\"boxed=%d\\n\", [t boxed]);
	return 0;
}}
",
        PREAMBLE(), ozq31_src()
    );
    let stdout = compile_and_run(&src, "number_basic_boxes_int_literal");
    assert_eq!(stdout, "boxed=42\n");
}

#[test]
fn q31_basic_roundtrip_and_arithmetic() {
    // q31_basic.m, all 13 methods.
    //
    // The `oz-pool` directive is needed because every Q31 here comes from
    // a single `[OZQ31 alloc]` site inside a factory method, so counting
    // sites sizes the slab at 1 while the test needs many live at once.
    // The oracle's own case carries `OZQ31=16` for exactly that reason.
    //
    // The count here is higher than the oracle's, though, and that gap is
    // structural rather than a tuning choice: the oracle has scope-based
    // ARC, so each temporary Q31 is released at the end of the method
    // that made it and 16 slots recirculate. oz_static has no ARC (#189),
    // so nothing releases these temporaries and every one allocated over
    // the whole run stays live. Any pool size ported from an oracle case
    // has to be raised for the same reason until ARC lands.
    let src = format!(
        "/* oz-pool: OZObject=1,OZQ31=64 */\n{}{}\n\
@interface FPTest : OZObject
- (int)intFromLiteral;
- (float)floatFromLiteral;
- (int)intFromExpr;
- (int)int8Roundtrip;
- (int)uint16Roundtrip;
- (int)boolTrue;
- (int)boolFalse;
- (int)rawNonZero;
- (int)shiftForTen;
- (int)addResult;
- (int)subResult;
- (int)mulResult;
- (float)divResult;
@end

@implementation FPTest
- (int)intFromLiteral {{
	OZQ31 *n = @42;
	int v = [n intValue];
	return v;
}}
- (float)floatFromLiteral {{
	OZQ31 *n = @(3.5f);
	float v = [n floatValue];
	return v;
}}
- (int)intFromExpr {{
	int x = 7;
	OZQ31 *n = @(x + 3);
	int v = [n int32Value];
	return v;
}}
- (int)int8Roundtrip {{
	OZQ31 *n = @(100);
	int v = [n int8Value];
	return v;
}}
- (int)uint16Roundtrip {{
	OZQ31 *n = @(1000);
	int v = [n uint16Value];
	return v;
}}
- (int)boolTrue {{
	OZQ31 *n = @(42);
	int v = [n boolValue];
	return v;
}}
- (int)boolFalse {{
	OZQ31 *n = @(0);
	int v = [n boolValue];
	return v;
}}
- (int)rawNonZero {{
	OZQ31 *n = @(5);
	int v = [n rawValue] != 0;
	return v;
}}
- (int)shiftForTen {{
	OZQ31 *n = @(10);
	int v = [n shift];
	return v;
}}
- (int)addResult {{
	OZQ31 *a = @(10);
	OZQ31 *b = @(20);
	OZQ31 *c = [a add:b];
	int v = [c int32Value];
	return v;
}}
- (int)subResult {{
	OZQ31 *a = @(50);
	OZQ31 *b = @(20);
	OZQ31 *c = [a sub:b];
	int v = [c int32Value];
	return v;
}}
- (int)mulResult {{
	OZQ31 *a = @(6);
	OZQ31 *b = @(7);
	OZQ31 *c = [a mul:b];
	int v = [c int32Value];
	return v;
}}
- (float)divResult {{
	OZQ31 *a = @(10);
	OZQ31 *b = @(4);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}}
@end

#include <stdio.h>
int main(void) {{
	FPTest *t = [FPTest alloc];
	printf(\"intFromLiteral=%d\\n\", [t intFromLiteral]);
	printf(\"floatFromLiteral=%.2f\\n\", [t floatFromLiteral]);
	printf(\"intFromExpr=%d\\n\", [t intFromExpr]);
	printf(\"int8Roundtrip=%d\\n\", [t int8Roundtrip]);
	printf(\"uint16Roundtrip=%d\\n\", [t uint16Roundtrip]);
	printf(\"boolTrue=%d\\n\", [t boolTrue]);
	printf(\"boolFalse=%d\\n\", [t boolFalse]);
	printf(\"rawNonZero=%d\\n\", [t rawNonZero]);
	printf(\"shiftForTen=%d\\n\", [t shiftForTen]);
	printf(\"addResult=%d\\n\", [t addResult]);
	printf(\"subResult=%d\\n\", [t subResult]);
	printf(\"mulResult=%d\\n\", [t mulResult]);
	printf(\"divResult=%.2f\\n\", [t divResult]);
	return 0;
}}
",
        PREAMBLE(), ozq31_src()
    );
    let stdout = compile_and_run(&src, "q31_basic_roundtrip_and_arithmetic");
    assert_eq!(
        stdout,
        "intFromLiteral=42\n\
         floatFromLiteral=3.50\n\
         intFromExpr=10\n\
         int8Roundtrip=100\n\
         uint16Roundtrip=1000\n\
         boolTrue=1\n\
         boolFalse=0\n\
         rawNonZero=1\n\
         shiftForTen=4\n\
         addResult=30\n\
         subResult=30\n\
         mulResult=42\n\
         divResult=2.50\n"
    );
}

#[test]
fn q31_stdio_free_to_str_and_div_helpers() {
    // q31_stdio_free.m's oracle calls _oz_q31_to_str/_oz_q31_div directly
    // as plain C (not through ObjC) for most of its ~40 assertions -- so
    // this ports those directly too, plus the few Q31NoStdio class
    // methods that do go through -div:.
    let src = format!(
        "{}{}\n\
static void check_str(const char *label, int32_t raw, uint8_t shift, int precision, const char *expected) {{
	char buf[32];
	int n = _oz_q31_to_str(raw, shift, buf, sizeof(buf), precision);
	if (n < (int)sizeof(buf)) {{
		buf[n] = 0;
	}}
	int ok = 1;
	const char *a = buf;
	const char *b = expected;
	while (*a && *b) {{
		if (*a != *b) {{
			ok = 0;
		}}
		a++;
		b++;
	}}
	if (*a != *b) {{
		ok = 0;
	}}
	printf(\"%s=%s(%s)\\n\", label, ok ? \"ok\" : \"FAIL\", buf);
}}

@interface Q31NoStdio : OZObject
- (float)divTenByFour;
- (int)divByZeroRaw;
@end
@implementation Q31NoStdio
- (float)divTenByFour {{
	OZQ31 *a = @(10);
	OZQ31 *b = @(4);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}}
- (int)divByZeroRaw {{
	OZQ31 *a = @(10);
	OZQ31 *b = [OZQ31 fixedWithRaw:0 shift:0];
	OZQ31 *c = [a div:b];
	int v = [c rawValue];
	return v;
}}
@end

#include <stdio.h>
int main(void) {{
	check_str(\"zero\", 0, 0, 14, \"0\");
	check_str(\"one\", 1 << 30, 1, 14, \"1\");
	check_str(\"ten\", 10 << 27, 4, 14, \"10\");
	check_str(\"hundred\", 100 << 24, 7, 14, \"100\");
	check_str(\"thousand\", 1000 << 21, 10, 14, \"1000\");
	check_str(\"neg_one\", -(1 << 30), 1, 14, \"-1\");
	check_str(\"half\", 1073741824, 0, 14, \"0.5\");
	check_str(\"quarter\", 536870912, 0, 14, \"0.25\");
	check_str(\"three_half\", 1879048192, 2, 14, \"3.5\");
	check_str(\"ten_half\", 1409286144, 4, 14, \"10.5\");
	check_str(\"one_third_14\", 715827882, 0, 14, \"0.33333333302289\");
	check_str(\"two_third_14\", 1431655765, 0, 14, \"0.66666666651145\");
	check_str(\"neg_half\", -1073741824, 0, 14, \"-0.5\");
	check_str(\"shift31_int\", 42, 31, 14, \"42\");
	check_str(\"shift31_neg\", -99, 31, 14, \"-99\");
	check_str(\"smallest_pos\", 1, 0, 14, \"0.00000000046566\");
	check_str(\"near_one\", 2147483647, 0, 14, \"0.99999999953434\");
	check_str(\"shift31_large\", 1000000, 31, 14, \"1000000\");
	check_str(\"one_eighth\", 268435456, 0, 14, \"0.125\");
	check_str(\"shift30\", 3, 30, 14, \"1.5\");
	check_str(\"all_nines\", 2147483537, 0, 14, \"0.9999999483116\");
	check_str(\"pow_of_two\", 256 << 22, 9, 14, \"256\");
	check_str(\"prec0\", 715827882, 0, 0, \"0\");
	check_str(\"prec1\", 715827882, 0, 1, \"0.3\");
	check_str(\"prec4\", 715827882, 0, 4, \"0.3333\");
	check_str(\"prec6\", 715827882, 0, 6, \"0.333333\");
	check_str(\"prec6_round\", 1431655765, 0, 6, \"0.666667\");
	check_str(\"prec_neg_clamped\", 1073741824, 0, -5, \"0\");
	check_str(\"prec_over_clamped\", 715827882, 0, 20, \"0.33333333302289\");
	check_str(\"prec6_carry\", 2147483647, 0, 6, \"1\");
	check_str(\"prec6_smallest\", 1, 0, 6, \"0\");

	{{
		int32_t r_raw;
		uint8_t r_shift;
		_oz_q31_div(10 << 27, 4, 2 << 29, 2, &r_raw, &r_shift);
		int32_t iv = (r_shift >= 31) ? r_raw : (r_raw >> (31 - r_shift));
		printf(\"div_exact=%s\\n\", iv == 5 ? \"ok\" : \"FAIL\");
	}}
	{{
		int32_t r_raw;
		uint8_t r_shift;
		_oz_q31_div(10 << 27, 4, 0, 0, &r_raw, &r_shift);
		printf(\"div_by_zero=%s\\n\", (r_raw == 0 && r_shift == 0) ? \"ok\" : \"FAIL\");
	}}
	{{
		int32_t r_raw;
		uint8_t r_shift;
		_oz_q31_div(-(10 << 27), 4, -(2 << 29), 2, &r_raw, &r_shift);
		int32_t iv = (r_shift >= 31) ? r_raw : (r_raw >> (31 - r_shift));
		printf(\"div_both_neg=%s\\n\", iv == 5 ? \"ok\" : \"FAIL\");
	}}

	Q31NoStdio *t = [Q31NoStdio alloc];
	float d = [t divTenByFour];
	printf(\"div_ten_by_four=%s\\n\", (d > 2.49f && d < 2.51f) ? \"ok\" : \"FAIL\");
	printf(\"div_by_zero_raw=%s\\n\", [t divByZeroRaw] == 0 ? \"ok\" : \"FAIL\");
	return 0;
}}
",
        PREAMBLE(), ozq31_src()
    );
    let stdout = compile_and_run(&src, "q31_stdio_free_to_str_and_div_helpers");
    for line in stdout.lines() {
        assert!(!line.contains("FAIL"), "failed check: {} (full output:\n{})", line, stdout);
    }
    // 31 check_str calls + 3 direct _oz_q31_div calls + 2 Q31NoStdio checks.
    assert_eq!(stdout.lines().count(), 36, "unexpected line count, full output:\n{}", stdout);
}
