// SPDX-License-Identifier: Apache-2.0
//
// behavior_forin.rs - OZ-092: port of tests/behavior/cases/forin/ (the
// Python-pipeline oracle) to oz_static. All 4 fixtures are in scope,
// unlocked by Foundation classes existing (the collection every one of
// them iterates is always a boxed OZArray literal).
//
// `for (Type *var in collection) { body }` lowers via
// `emit::render_forin_statement` to the exact iterator-based C for loop
// shape the Python oracle already uses (`_emit_forin_stmt`,
// tools/oz_transpile/emit.py): a scoped block calling `-iter` once, then
// `-next` per iteration, both routed through `OZ_PROTOCOL_SEND_` (see
// the dynamic-dispatch generalization from the OZDictionary work) since
// the collection's static type could be anywhere from a concrete class
// to plain `id`. `break`/nesting need no special handling -- this
// desugars to a real C `for` loop wrapped in a block, so they already
// mean what they need to.
//
// Uses the real `OZObject` (`common::ozobject_src`) as the root class,
// `OZQ31`/`OZString` (`common::ozq31_src`/`common::ozstring_src`) for
// the boxed elements, `OZArray` (`common::ozarray_src`) as the
// collection, and `IteratorProtocol` (`common::iterator_protocol_src`)
// -- declaring it (not formal `<IteratorProtocol>` conformance) is what
// makes `-iter`/`-next` dynamically dispatchable at all.

mod common;
use common::{
    compile_and_run, iterator_protocol_src, ozarray_src, ozobject_src as PREAMBLE, ozq31_src, ozstring_src,
};

#[test]
fn basic_array_sums_elements() {
    // basic_array.m: sums every OZQ31 element of a 3-item literal via
    // for-in.
    let src = format!(
        "{}{}{}{}\n\
@interface IterTest : OZObject {{
	int _sum;
}}
- (void)sumArray;
- (int)sum;
@end

@implementation IterTest
- (void)sumArray {{
	OZArray *arr = @[@(10), @(20), @(30)];
	_sum = 0;
	for (OZQ31 *n in arr) {{
		_sum = _sum + [n intValue];
	}}
}}
- (int)sum {{
	return _sum;
}}
@end

#include <stdio.h>
int main(void) {{
	IterTest *t = [IterTest alloc];
	[t sumArray];
	printf(\"sum=%d\\n\", [t sum]);
	return 0;
}}
",
        PREAMBLE(),
        iterator_protocol_src(),
        ozq31_src(),
        ozarray_src()
    );
    let stdout = compile_and_run(&src, "basic_array_sums_elements");
    assert_eq!(stdout, "sum=60\n");
}

#[test]
fn break_in_forin_stops_early() {
    // break_in_forin.m: stops accumulating as soon as the element hits
    // 3 -- the last value recorded before the break is 2, proving a
    // plain `break` inside the desugared C for loop works exactly as a
    // real C for loop's would.
    let src = format!(
        "{}{}{}{}\n\
@interface BreakIterTest : OZObject {{
	int _stoppedAt;
}}
- (void)breakAtThreshold;
- (int)stoppedAt;
@end

@implementation BreakIterTest
- (void)breakAtThreshold {{
	OZArray *arr = @[@(1), @(2), @(3), @(4)];
	_stoppedAt = 0;
	for (OZQ31 *n in arr) {{
		int v = [n intValue];
		if (v == 3) {{
			break;
		}}
		_stoppedAt = v;
	}}
}}
- (int)stoppedAt {{
	return _stoppedAt;
}}
@end

#include <stdio.h>
int main(void) {{
	BreakIterTest *t = [BreakIterTest alloc];
	[t breakAtThreshold];
	printf(\"stoppedAt=%d\\n\", [t stoppedAt]);
	return 0;
}}
",
        PREAMBLE(),
        iterator_protocol_src(),
        ozq31_src(),
        ozarray_src()
    );
    let stdout = compile_and_run(&src, "break_in_forin_stops_early");
    assert_eq!(stdout, "stoppedAt=2\n");
}

#[test]
fn nested_forin_computes_correctly() {
    // nested_forin.m: two independent for-in loops, nested -- each
    // desugars to its own scoped block/iterator pair, so they don't
    // interfere: outer [1,2] x inner [10,20] -> (1+10)+(1+20)+(2+10)+(2+20) = 66.
    let src = format!(
        "{}{}{}{}\n\
@interface NestedIterTest : OZObject {{
	int _total;
}}
- (void)nestedIteration;
- (int)total;
@end

@implementation NestedIterTest
- (void)nestedIteration {{
	OZArray *outer = @[@(1), @(2)];
	OZArray *inner = @[@(10), @(20)];
	_total = 0;
	for (OZQ31 *a in outer) {{
		for (OZQ31 *b in inner) {{
			_total = _total + [a intValue] + [b intValue];
		}}
	}}
}}
- (int)total {{
	return _total;
}}
@end

#include <stdio.h>
int main(void) {{
	NestedIterTest *t = [NestedIterTest alloc];
	[t nestedIteration];
	printf(\"total=%d\\n\", [t total]);
	return 0;
}}
",
        PREAMBLE(),
        iterator_protocol_src(),
        ozq31_src(),
        ozarray_src()
    );
    let stdout = compile_and_run(&src, "nested_forin_computes_correctly");
    assert_eq!(stdout, "total=66\n");
}

#[test]
fn typed_var_iterates_ozstring() {
    // typed_var.m: for-in with a non-numeric element type (OZString *)
    // -- proves the loop variable's declared type isn't special-cased
    // to OZQ31.
    let src = format!(
        "{}{}{}{}\n\
@interface TypedIterTest : OZObject {{
	int _count;
}}
- (void)countStrings;
- (size_t)count;
@end

@implementation TypedIterTest
- (void)countStrings {{
	OZArray *arr = @[@\"hello\", @\"world\", @\"oz\"];
	_count = 0;
	for (OZString *s in arr) {{
		if ([s length] > 0) {{
			_count = _count + 1;
		}}
	}}
}}
- (size_t)count {{
	return _count;
}}
@end

#include <stdio.h>
int main(void) {{
	TypedIterTest *t = [TypedIterTest alloc];
	[t countStrings];
	printf(\"count=%d\\n\", [t count]);
	return 0;
}}
",
        PREAMBLE(),
        iterator_protocol_src(),
        ozstring_src(),
        ozarray_src()
    );
    let stdout = compile_and_run(&src, "typed_var_iterates_ozstring");
    assert_eq!(stdout, "count=3\n");
}
