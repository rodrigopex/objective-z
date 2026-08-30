// SPDX-License-Identifier: Apache-2.0
//
// behavior_category.rs - categories (`@interface Foo (Cat)` /
// `@implementation Foo (Cat)`).
//
// New coverage: the oracle's tests/behavior/cases/ has no category case,
// so there was nothing to port and nothing pinning the behavior on either
// side.
//
// Method *bodies* in a category already worked before this file existed:
// `collect`'s pass 1 skips a category interface (it declares no new
// class), but its pass 2 `class_implementation` arm never looked at the
// category name, so `@implementation Foo (Cat)`'s methods were always
// collected onto `Foo`. What did not work was a category-declared
// `@property` -- pass 2 skipped category *interfaces* wholesale, so the
// property was never collected and any use of its accessors failed with
// "class 'Foo' has no method matching 'setSlot2:'".

mod common;
use common::{compile_and_run, ozobject_src};

#[test]
fn category_method_bodies_reach_ivars_and_siblings() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Greeter : OZObject {
	int _n;
}
- (int)base;
@end
@implementation Greeter
- (int)base {
	return 1;
}
@end

@interface Greeter (Extra)
- (int)extra;
- (int)extraPlus:(int)k;
@end

@implementation Greeter (Extra)
- (int)extra {
	return 40 + [self base];
}
- (int)extraPlus:(int)k {
	_n = k;
	return _n + [self extra];
}
@end

#include <stdio.h>
int main(void) {
	Greeter *g = [Greeter alloc];
	printf(\"base=%d\\n\", [g base]);
	printf(\"extra=%d\\n\", [g extra]);
	printf(\"extraPlus=%d\\n\", [g extraPlus:2]);
	[g release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "category_method_bodies_reach_ivars_and_siblings");
    assert_eq!(stdout, "base=1\nextra=41\nextraPlus=43\n");
}

/// A `@property` declared in a category. Its accessors are synthesized
/// from the class's *primary* @implementation only -- the property merges
/// into the class, so every @implementation block for that class can see
/// it, and synthesizing per block would emit the same C function twice
/// ("redefinition of 'Holder_slot2'").
#[test]
fn category_property_synthesizes_accessors_once() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Holder : OZObject {
	int _slot;
}
- (int)slot;
@end
@implementation Holder
- (int)slot {
	return _slot;
}
@end

@interface Holder (Props)
@property (nonatomic) int slot2;
@end

@implementation Holder (Props)
@end

#include <stdio.h>
int main(void) {
	Holder *h = [Holder alloc];
	printf(\"slot2_initial=%d\\n\", [h slot2]);
	[h setSlot2:7];
	printf(\"slot2=%d\\n\", [h slot2]);
	printf(\"slot_untouched=%d\\n\", [h slot]);
	[h release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "category_property_synthesizes_accessors_once");
    assert_eq!(stdout, "slot2_initial=0\nslot2=7\nslot_untouched=0\n");
}

/// A category may restate a selector the main @interface already
/// declared; the merge deduplicates rather than emitting two prototypes.
#[test]
fn category_restating_a_declared_selector_is_deduplicated() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Dup : OZObject
- (int)val;
@end
@implementation Dup
- (int)val {
	return 5;
}
@end

@interface Dup (Again)
- (int)val;
- (int)twice;
@end
@implementation Dup (Again)
- (int)twice {
	return [self val] * 2;
}
@end

#include <stdio.h>
int main(void) {
	Dup *d = [Dup alloc];
	printf(\"val=%d\\n\", [d val]);
	printf(\"twice=%d\\n\", [d twice]);
	[d release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "category_restating_a_declared_selector_is_deduplicated");
    assert_eq!(stdout, "val=5\ntwice=10\n");
}
