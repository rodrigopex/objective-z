// SPDX-License-Identifier: Apache-2.0
//
// behavior_immortal_literals.rs - a boxed string literal (`@"..."`) lives
// in static storage, so it must never be passed to free().
//
// Releasing it does happen in ordinary code: a collection that absorbed a
// literal releases its elements when it is itself deallocated, so a
// literal's refcount really does reach zero. `companion`'s release path
// calls `{class}_oz_free` at zero, which for OZString is `free(obj)` --
// on a static, that aborts. `emit::render_boxed_string_literal` marks
// literals `_meta.deallocating = 1` from birth so `oz_static_release`
// returns before the free switch.
//
// This is what made a dictionary literal abort on release while an array
// literal released cleanly: dictionary *keys* here are string literals,
// whereas `@[ @10, @20 ]`'s elements are heap-allocated OZQ31 boxes.

mod common;
use common::{
    compile_and_run, iterator_protocol_src, ozarray_src, ozdictionary_src, ozobject_src,
    ozq31_src, ozstring_src,
};

#[test]
fn releasing_dictionary_literal_with_string_keys_does_not_abort() {
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
	printf(\"count=%u\\n\", [scores count]);
	[scores release];
	printf(\"released_ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "releasing_dictionary_literal_with_string_keys");
    assert_eq!(stdout, "count=2\nreleased_ok\n");
}

#[test]
fn releasing_array_of_string_literals_does_not_abort() {
    let src = format!(
        "{}{}{}{}{}\n{}",
        ozobject_src(),
        iterator_protocol_src(),
        ozq31_src(),
        ozstring_src(),
        ozarray_src(),
        "\
#include <stdio.h>
int main(void) {
	OZArray *names = @[ @\"zephyr\", @\"objective-z\" ];
	printf(\"count=%u\\n\", [names count]);
	[names release];
	printf(\"released_ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "releasing_array_of_string_literals");
    assert_eq!(stdout, "count=2\nreleased_ok\n");
}

/// A literal released directly, to its refcount floor, then still used --
/// the storage must survive, since nothing may free it.
#[test]
fn literal_survives_release_to_zero() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        ozstring_src(),
        "\
#include <stdio.h>
int main(void) {
	OZString *s = @\"hello\";
	printf(\"len_before=%u\\n\", [s length]);
	[s release];
	printf(\"cstr_after=%s\\n\", [s cString]);
	printf(\"len_after=%u\\n\", [s length]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "literal_survives_release_to_zero");
    assert_eq!(stdout, "len_before=5\ncstr_after=hello\nlen_after=5\n");
}

/// A heap-allocated OZString (not a literal) must still be freed
/// normally -- the immortality marker applies only to literals.
#[test]
fn heap_allocated_object_still_freed_normally() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Counted : OZObject {
	int _n;
}
- (int)n;
@end
@implementation Counted
- (int)n {
	return _n;
}
@end

#include <stdio.h>
int main(void) {
	Counted *c = [Counted alloc];
	printf(\"rc=%d\\n\", [c retainCount]);
	[c retain];
	printf(\"rc2=%d\\n\", [c retainCount]);
	[c release];
	printf(\"rc3=%d\\n\", [c retainCount]);
	[c release];
	printf(\"freed_ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "heap_allocated_object_still_freed_normally");
    assert_eq!(stdout, "rc=1\nrc2=2\nrc3=1\nfreed_ok\n");
}
