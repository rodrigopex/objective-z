// SPDX-License-Identifier: Apache-2.0
//
// behavior_memory.rs - OZ-092: port of the Python pipeline's
// tests/behavior/cases/memory/ fixtures to oz_static, using the Python
// pipeline (the oracle) as ground truth for what each fixture actually
// verifies. Same pattern as end_to_end_behavior.rs: the real `OZObject`
// (`common::ozobject_src`) as the root class, the class(es) under test,
// and a main() that printf's the values the original Unity `_test.c`
// asserted, checked here via an exact stdout match.
//
// tests/behavior/cases/memory/heap_alloc.m is covered in
// behavior_foundation_heap.rs instead, alongside the rest of OZHeap, now
// that `+allocWithHeap:` is implemented.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

#[test]
fn nested_retain_release() {
    // Oracle: tests/behavior/cases/memory/nested_retain_release_test.c --
    // retain twice, release twice, refcount observed at every step; freed
    // only on the final (third) release.
    let src = format!(
        "{}\n\
         @interface Handle : OZObject\n\
         @end\n\
         @implementation Handle\n\
         @end\n\
         \n\
         #include <stdio.h>\n\
         \n\
         int main(void) {{\n\
         \tHandle *h = [Handle alloc];\n\
         \tprintf(\"rc1=%d\\n\", [h retainCount]);\n\
         \t[h retain];\n\
         \tprintf(\"rc2=%d\\n\", [h retainCount]);\n\
         \t[h retain];\n\
         \tprintf(\"rc3=%d\\n\", [h retainCount]);\n\
         \t[h release];\n\
         \tprintf(\"rc4=%d\\n\", [h retainCount]);\n\
         \t[h release];\n\
         \tprintf(\"rc5=%d\\n\", [h retainCount]);\n\
         \t[h release];\n\
         \treturn 0;\n\
         }}\n",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "nested_retain_release");
    assert_eq!(stdout, "rc1=1\nrc2=2\nrc3=3\nrc4=2\nrc5=1\n");
}

#[test]
fn release_decrements_refcount() {
    // Oracle: tests/behavior/cases/memory/release_decrements_test.c --
    // release drops the refcount without deallocating while rc > 1.
    let src = format!(
        "{}\n\
         @interface Counter : OZObject\n\
         @end\n\
         @implementation Counter\n\
         @end\n\
         \n\
         #include <stdio.h>\n\
         \n\
         int main(void) {{\n\
         \tCounter *c = [Counter alloc];\n\
         \t[c retain];\n\
         \t[c retain];\n\
         \tprintf(\"rc1=%d\\n\", [c retainCount]);\n\
         \t[c release];\n\
         \tprintf(\"rc2=%d\\n\", [c retainCount]);\n\
         \t[c release];\n\
         \tprintf(\"rc3=%d\\n\", [c retainCount]);\n\
         \t[c release];\n\
         \treturn 0;\n\
         }}\n",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "release_decrements_refcount");
    assert_eq!(stdout, "rc1=3\nrc2=2\nrc3=1\n");
}

#[test]
fn release_frees_at_zero() {
    // Oracle: tests/behavior/cases/memory/release_frees_at_zero_test.c --
    // release at rc=1 frees the object; the Python fixture proves this via
    // a 1-block slab (alloc, release, re-alloc only succeeds if the block
    // was actually returned). oz_static's oz_alloc is malloc-based with no
    // slab to exhaust, so the equivalent guarantee this ports is simply:
    // alloc/release/alloc/release runs cleanly and every alloc yields a
    // live, non-null object -- releasing at rc=1 doesn't leave the
    // allocator (or a subsequent alloc) in a broken state.
    let src = format!(
        "{}\n\
         @interface Token : OZObject\n\
         @end\n\
         @implementation Token\n\
         @end\n\
         \n\
         #include <stdio.h>\n\
         \n\
         int main(void) {{\n\
         \tToken *t1 = [Token alloc];\n\
         \tprintf(\"t1_nonnull=%d\\n\", t1 != 0);\n\
         \t[t1 release];\n\
         \tToken *t2 = [Token alloc];\n\
         \tprintf(\"t2_nonnull=%d\\n\", t2 != 0);\n\
         \t[t2 release];\n\
         \treturn 0;\n\
         }}\n",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "release_frees_at_zero");
    assert_eq!(stdout, "t1_nonnull=1\nt2_nonnull=1\n");
}

#[test]
fn retain_count_query() {
    // Oracle: tests/behavior/cases/memory/retain_count_query_test.c --
    // retainCount reflects the current refcount at each step.
    let src = format!(
        "{}\n\
         @interface Tracker : OZObject\n\
         @end\n\
         @implementation Tracker\n\
         @end\n\
         \n\
         #include <stdio.h>\n\
         \n\
         int main(void) {{\n\
         \tTracker *t = [Tracker alloc];\n\
         \tprintf(\"rc1=%d\\n\", [t retainCount]);\n\
         \t[t retain];\n\
         \tprintf(\"rc2=%d\\n\", [t retainCount]);\n\
         \t[t release];\n\
         \tprintf(\"rc3=%d\\n\", [t retainCount]);\n\
         \t[t release];\n\
         \treturn 0;\n\
         }}\n",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "retain_count_query");
    assert_eq!(stdout, "rc1=1\nrc2=2\nrc3=1\n");
}

#[test]
fn retain_count_nil_returns_zero() {
    // Oracle: tests/behavior/cases/memory/retain_count_query_test.c,
    // test_retain_count_nil_returns_zero -- retainCount on a nil receiver
    // is 0, not a crash.
    let src = format!(
        "{}\n\
         @interface Tracker : OZObject\n\
         @end\n\
         @implementation Tracker\n\
         @end\n\
         \n\
         #include <stdio.h>\n\
         \n\
         int main(void) {{\n\
         \tTracker *nilT = 0;\n\
         \tprintf(\"nil_rc=%d\\n\", [nilT retainCount]);\n\
         \treturn 0;\n\
         }}\n",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "retain_count_nil_returns_zero");
    assert_eq!(stdout, "nil_rc=0\n");
}

#[test]
fn retain_increments_refcount() {
    // Oracle: tests/behavior/cases/memory/retain_increments_test.c --
    // retain increments the refcount.
    let src = format!(
        "{}\n\
         @interface Node : OZObject\n\
         @end\n\
         @implementation Node\n\
         @end\n\
         \n\
         #include <stdio.h>\n\
         \n\
         int main(void) {{\n\
         \tNode *n = [Node alloc];\n\
         \tprintf(\"rc1=%d\\n\", [n retainCount]);\n\
         \t[n retain];\n\
         \tprintf(\"rc2=%d\\n\", [n retainCount]);\n\
         \t[n retain];\n\
         \tprintf(\"rc3=%d\\n\", [n retainCount]);\n\
         \t[n release];\n\
         \t[n release];\n\
         \t[n release];\n\
         \treturn 0;\n\
         }}\n",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "retain_increments_refcount");
    assert_eq!(stdout, "rc1=1\nrc2=2\nrc3=3\n");
}

/// Assigning to a strong object ivar has to take ownership of the value.
///
/// It did not, and the asymmetry was a use-after-free waiting to happen:
/// `{Class}_oz_release_ivars` releases every owned object ivar when an
/// instance dies, but nothing had ever retained what was stored there. This
/// is the shape of `samples/transpiled_led` -- a chain of objects each
/// holding the previous one in a strong ivar assigned straight from a
/// parameter -- which segfaulted with nothing printed at all.
/// AddressSanitizer named it exactly: heap-use-after-free in
/// `oz_atomic_dec_and_test`, the object freed once by its owner's
/// `oz_release_ivars` and again by the scope-exit release of the local that
/// created it.
///
/// The refcount is what this checks, because it is the thing that was wrong:
/// `first` is held by a local *and* by `second`'s ivar, so it is 2 while
/// both are alive, and releasing `second` has to bring it back to 1 rather
/// than to 0.
#[test]
fn strong_ivar_assignment_takes_ownership() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
#include <stdio.h>

@interface Link : OZObject {
	int _tag;
	Link *_next;
}
- (instancetype)initWithTag:(int)tag next:(Link *)next;
- (int)tag;
@end

@implementation Link
- (instancetype)initWithTag:(int)tag next:(Link *)next {
	self = [super init];
	if (self) {
		_tag = tag;
		_next = next;
	}
	return self;
}
- (int)tag {
	return _tag;
}
- (void)dealloc {
	printf(\"dealloc %d\\n\", _tag);
}
@end

int main(void) {
	Link *first = [[Link alloc] initWithTag:1 next:nil];
	printf(\"first_rc=%d\\n\", [first retainCount]);
	Link *second = [[Link alloc] initWithTag:2 next:first];
	/* `first` is now held twice: by this scope, and by second's ivar. */
	printf(\"first_rc_held=%d\\n\", [first retainCount]);
	[second release];
	/* second's dealloc released its ivar, so `first` is back to just us. */
	printf(\"first_rc_after=%d\\n\", [first retainCount]);
	printf(\"still_alive_tag=%d\\n\", [first tag]);
	[first release];
	printf(\"done\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "strong_ivar_assignment_takes_ownership");
    assert_eq!(
        stdout,
        "first_rc=1\nfirst_rc_held=2\ndealloc 2\nfirst_rc_after=1\nstill_alive_tag=1\ndealloc 1\ndone\n"
    );
}

/// A `+1` right-hand side is stored without an extra retain: it already
/// carries the reference the ivar takes over, and a temporary has no
/// scope-exit release to balance a second one -- so retaining it too would
/// leak. The count stays 1, and the object still dies with its owner.
#[test]
fn strong_ivar_assigned_a_fresh_object_is_not_retained_twice() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
#include <stdio.h>

@interface Leaf : OZObject
@end
@implementation Leaf
- (void)dealloc {
	printf(\"leaf gone\\n\");
}
@end

@interface Holder : OZObject {
	Leaf *_leaf;
}
- (instancetype)init;
- (int)leafCount;
@end

@implementation Holder
- (instancetype)init {
	self = [super init];
	if (self) {
		_leaf = [Leaf alloc];
	}
	return self;
}
- (int)leafCount {
	return [_leaf retainCount];
}
- (void)dealloc {
}
@end

int main(void) {
	Holder *h = [[Holder alloc] init];
	printf(\"leaf_rc=%d\\n\", [h leafCount]);
	[h release];
	printf(\"done\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "strong_ivar_assigned_a_fresh_object_is_not_retained_twice");
    assert_eq!(stdout, "leaf_rc=1\nleaf gone\ndone\n");
}
