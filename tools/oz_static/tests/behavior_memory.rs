// SPDX-License-Identifier: Apache-2.0
//
// behavior_memory.rs - OZ-092: port of the Python pipeline's
// tests/behavior/cases/memory/ fixtures to oz_static, using the Python
// pipeline (the oracle) as ground truth for what each fixture actually
// verifies. Same pattern as end_to_end_behavior.rs: a synthetic OZSRoot
// preamble (oz_static has no shared Foundation root yet), the class(es)
// under test, and a main() that printf's the values the original Unity
// `_test.c` asserted, checked here via an exact stdout match.
//
// tests/behavior/cases/memory/heap_alloc.m is NOT ported here: it exercises
// the Python pipeline's `allocWithHeap:` (a heap-backed OZHeap allocation
// variant, distinct from slab alloc, with its own usage-tracking API).
// oz_static only synthesizes one malloc-based `{Class}_oz_alloc` -- there is
// no heap-backed allocation variant to test an equivalent of. Not tracked as
// its own issue; falls under OZ-092's (#190) general note that
// Foundation-adjacent features are Phase 2.

mod common;
use common::compile_and_run;

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
fn nested_retain_release() {
    // Oracle: tests/behavior/cases/memory/nested_retain_release_test.c --
    // retain twice, release twice, refcount observed at every step; freed
    // only on the final (third) release.
    let src = format!(
        "{}\n\
         @interface Handle : OZSRoot\n\
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
        PREAMBLE
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
         @interface Counter : OZSRoot\n\
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
        PREAMBLE
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
         @interface Token : OZSRoot\n\
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
        PREAMBLE
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
         @interface Tracker : OZSRoot\n\
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
        PREAMBLE
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
         @interface Tracker : OZSRoot\n\
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
        PREAMBLE
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
         @interface Node : OZSRoot\n\
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
        PREAMBLE
    );
    let stdout = compile_and_run(&src, "retain_increments_refcount");
    assert_eq!(stdout, "rc1=1\nrc2=2\nrc3=3\n");
}
