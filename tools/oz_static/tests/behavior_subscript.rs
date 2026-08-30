// SPDX-License-Identifier: Apache-2.0
//
// behavior_subscript.rs - ObjC subscripting: `array[0]` and
// `dict[@"key"]` desugared to -objectAtIndexedSubscript: /
// -objectForKeyedSubscript: (see `emit::render_subscript_expression`).
//
// This was a silent gap, not a rejected construct: a subscript on an
// object used to pass straight through as C array indexing over the
// object pointer. `samples/transpiled_generics/src/main.m` -- which the
// Python backend builds today -- relies on all of `numbers[0]`,
// `scores[@"alpha"]`, and nested `matrix[0]` / `firstRow[1]`, so this is
// required for that sample to build under the static backend at all.
//
// The oracle has no dedicated subscript behavior case (its
// tests/behavior/cases/foundation/ suite reaches elements through
// -objectAtIndex: / -objectForKey: instead), so the coverage here is new
// on both sides; the sample above is what pins the feature.

mod common;
use common::{
    compile_and_run, expect_reject, iterator_protocol_src, ozarray_src, ozdictionary_src,
    ozobject_src, ozq31_src, ozstring_src,
};

#[test]
fn indexed_subscript_reads_array_element() {
    let src = format!(
        "{}{}{}{}\n{}",
        ozobject_src(),
        iterator_protocol_src(),
        ozq31_src(),
        ozarray_src(),
        "\
#include <stdio.h>
int main(void) {
	OZArray *nums = @[ @10, @20, @30 ];
	OZQ31 *first = nums[0];
	OZQ31 *third = nums[2];
	printf(\"first=%d\\n\", [first int32Value]);
	printf(\"third=%d\\n\", [third int32Value]);
	printf(\"count=%d\\n\", [nums count]);
	[nums release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "indexed_subscript_reads_array_element");
    assert_eq!(stdout, "first=10\nthird=30\ncount=3\n");
}

/// Does not release the dictionary, matching
/// `behavior_foundation_dictionary`'s existing idiom: releasing a
/// dictionary literal currently aborts, because OZDictionary has no
/// collection dealloc in oz_static (the dispatch switch routes its
/// dealloc to the plain root one, unlike the oracle's dedicated
/// element-releasing dealloc). That is a separate pre-existing gap, not
/// something subscripting introduced -- an *array* literal releases
/// cleanly, and this test's keyed reads are correct either way.
#[test]
fn keyed_subscript_reads_dictionary_value() {
    let src = format!(
        "{}{}{}{}{}\n{}",
        ozobject_src(),
        iterator_protocol_src(),
        ozq31_src(),
        ozstring_src(),
        ozdictionary_src(),
        "\
#include <stdio.h>
int main(void) {
	OZDictionary *scores = @{ @\"alpha\" : @100, @\"beta\" : @200 };
	OZQ31 *alpha = scores[@\"alpha\"];
	OZQ31 *beta = scores[@\"beta\"];
	printf(\"alpha=%d\\n\", [alpha int32Value]);
	printf(\"beta=%d\\n\", [beta int32Value]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "keyed_subscript_reads_dictionary_value");
    assert_eq!(stdout, "alpha=100\nbeta=200\n");
}

/// The shape `samples/transpiled_generics/src/main.m` uses: a subscript
/// whose result is itself subscripted.
#[test]
fn nested_subscript_on_array_of_arrays() {
    let src = format!(
        "{}{}{}{}\n{}",
        ozobject_src(),
        iterator_protocol_src(),
        ozq31_src(),
        ozarray_src(),
        "\
#include <stdio.h>
int main(void) {
	OZArray *row0 = @[ @1, @2 ];
	OZArray *row1 = @[ @3, @4 ];
	OZArray *matrix = @[ row0, row1 ];
	OZArray *firstRow = matrix[0];
	OZQ31 *m01 = firstRow[1];
	printf(\"m01=%d\\n\", [m01 int32Value]);
	OZArray *secondRow = matrix[1];
	OZQ31 *m10 = secondRow[0];
	printf(\"m10=%d\\n\", [m10 int32Value]);
	[matrix release];
	[row0 release];
	[row1 release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "nested_subscript_on_array_of_arrays");
    assert_eq!(stdout, "m01=2\nm10=3\n");
}

/// Plain C array indexing must be left alone -- the Foundation sources
/// themselves index raw buffers (`_items[index]` over an `id *_items`),
/// so only a *resolved object* receiver is rewritten into a message send.
#[test]
fn c_array_indexing_is_untouched() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Buf : OZObject {
	int _vals[4];
}
- (void)fill;
- (int)at:(int)i;
@end
@implementation Buf
- (void)fill {
	_vals[0] = 7;
	_vals[1] = 8;
}
- (int)at:(int)i {
	return _vals[i];
}
@end

#include <stdio.h>
int main(void) {
	int local[3];
	local[0] = 5;
	Buf *b = [Buf alloc];
	[b fill];
	printf(\"v0=%d\\n\", [b at:0]);
	printf(\"v1=%d\\n\", [b at:1]);
	printf(\"local0=%d\\n\", local[0]);
	[b release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "c_array_indexing_is_untouched");
    assert_eq!(stdout, "v0=7\nv1=8\nlocal0=5\n");
}

/// Subscripting an object whose class implements neither subscript
/// selector is a hard error. It used to pass through as pointer
/// arithmetic over the object -- which is meaningless, and only failed at
/// all because the resulting C happened not to typecheck.
#[test]
fn subscript_on_non_subscriptable_class_rejected() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Plain : OZObject
- (int)val;
@end
@implementation Plain
- (int)val {
	return 1;
}
@end

int main(void) {
	Plain *p = [Plain alloc];
	id x = p[0];
	return 0;
}
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("does not support subscripting"), "diagnostics: {}", diags);
    assert!(diags.contains("objectAtIndexedSubscript:"), "diagnostics: {}", diags);
}
