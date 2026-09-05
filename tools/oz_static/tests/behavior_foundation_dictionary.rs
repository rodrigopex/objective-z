// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_dictionary.rs - OZ-092 Foundation work:
// OZDictionary, ported from tests/behavior/cases/foundation/
// dictionary_basic.m.
//
// Uses the real `OZObject` (`common::ozobject_src`) as the root class,
// `OZQ31` (`common::ozq31_src`) for the boxed integer values, `OZString`
// (`common::ozstring_src`) for the boxed string keys, and `OZDictionary`
// (`common::ozdictionary_src`, a partial port -- see its doc comment).
// `@{...}` desugars via `emit::render_boxed_dictionary_literal`, the
// dictionary counterpart of the array literal desugar, into a call to
// the malloc-based `OZDictionary_oz_initWithKeysValues` builder
// (`companion::render_dict_support`). `-objectForKey:`'s `[k isEqual:key]`
// (`k` a bare `id` key) is what originally motivated generalizing
// oz_static's dynamic dispatch (see the dispatch-fix commit) -- without
// it, this whole fixture would be unportable.

mod common;
use common::{compile_and_run, ozdictionary_src, ozobject_src as PREAMBLE, ozq31_src, ozstring_src};

#[test]
fn dictionary_basic_literal_count_value_for_key_and_missing_key() {
    // dictionary_basic.m: literalCount (count of a 2-pair literal),
    // valueForKey (objectForKey:, unboxed via intValue), missingKeyNil
    // (objectForKey: for an absent key returns nil).
    let src = format!(
        "{}{}{}{}\n\
@interface DictTest : OZObject
- (unsigned int)literalCount;
- (int)valueForKey;
- (BOOL)missingKeyNil;
@end

@implementation DictTest
- (unsigned int)literalCount {{
	OZDictionary *d = @{{@\"a\": @(1), @\"b\": @(2)}};
	size_t c = [d count];
	return c;
}}
- (int)valueForKey {{
	OZDictionary *d = @{{@\"x\": @(99)}};
	OZQ31 *n = [d objectForKey:@\"x\"];
	int v = [n intValue];
	return v;
}}
- (BOOL)missingKeyNil {{
	OZDictionary *d = @{{@\"a\": @(1)}};
	id obj = [d objectForKey:@\"z\"];
	return obj == nil;
}}
@end

#include <stdio.h>
int main(void) {{
	DictTest *t = [DictTest alloc];
	printf(\"count=%zu\\n\", [t literalCount]);
	printf(\"value=%d\\n\", [t valueForKey]);
	printf(\"missing=%d\\n\", [t missingKeyNil]);
	return 0;
}}
",
        PREAMBLE(),
        ozstring_src(),
        ozq31_src(),
        ozdictionary_src()
    );
    let stdout = compile_and_run(&src, "dictionary_basic_literal_count_value_for_key_and_missing_key");
    assert_eq!(stdout, "count=2\nvalue=99\nmissing=1\n");
}
