// selector_ownership_matrix.rs -- which selectors and constructs create
// or consume a reference, one row each (#400).
//
// The dimension the four earlier audits did not walk. #359 walked every
// sink a `+1` can reach, #376 every position a `+1` expression can
// occupy, #345 what bounds an allocation in a loop, #385 what turning the
// AST on changes. This one walks the *set itself*: the selectors and
// constructs every other answer is built on.
//
// Probing it far enough to file #400 produced #398 -- a segfault on main
// from a selector that merely begins with `init`. Writing the matrix
// produced one more: an `id`-typed local holding a `+1` was never
// released, because `managed_object_locals` decided object-ness from the
// declared type's *spelling* and `id` carries no `*`.
//
// **Rows assert observed output, not refcount counts.** That is
// deliberate and it is the lesson of #376: its eager-allocation defect
// had matching retain/release counts and was invisible to a counting
// matrix. Only a side effect -- a `-dealloc` that prints -- catches it.
// `ownership_matrix.rs` counts, and counting is blind to *which* pointer
// a release names, which is exactly what #398 got wrong.
//
// Everything not listed here is believed correct, and that claim is only
// worth making if the list is complete. Adding a selector or construct
// that can create or consume a reference means adding a row.

mod common;

use common::{
        compile_and_run, compile_and_run_with_heap, compile_and_run_with_reflection,
        ozobject_src,
};

/// One cell: what it exercises, the program body, and the exact output
/// that program must produce.
struct Cell {
        /// The selector or construct, in the words #400 used.
        what: &'static str,
        /// `main`'s body. `Thing` and `Maker` below are in scope.
        body: &'static str,
        /// Exact stdout. A `-dealloc` that prints is what makes an
        /// unreleased reference visible.
        expect: &'static str,
        /// Set when reflection is needed (`@selector`, `-performSelector:`).
        needs_reflection: bool,
        /// Set when heap support is needed (`+dynamicAlloc`), which is off
        /// unless asked for -- without it the selector is a located error
        /// rather than a wrong answer, and the row would pass for the
        /// wrong reason.
        needs_heap: bool,
        /// `Some(issue)` when the expectation is the *defect* rather than
        /// the correct answer -- asserted so that fixing it fails here.
        known_defect: Option<&'static str>,
}

/// Every class the rows share. `-dealloc` prints so a missing release is
/// observable; `-copy` and `-mutableCopy` are declared here because the
/// SDK declares neither, which is the whole reason those two arms of the
/// owning set were untested before this file.
const DECLS: &str = "\
@interface Thing : OZObject
- (instancetype)copy;
- (instancetype)mutableCopy;
- (id)autorelease;
- (int)initialValue;
+ (instancetype)new;
@end
@implementation Thing
- (instancetype)copy { return [[Thing alloc] init]; }
- (instancetype)mutableCopy { return [[Thing alloc] init]; }
- (id)autorelease { return self; }
- (int)initialValue { return 42; }
+ (instancetype)new { return [[Thing alloc] init]; }
- (void)dealloc { printf(\"d\\n\"); }
@end

@interface Maker : OZObject
- (Thing *)copyOfOne;
@end
@implementation Maker
- (Thing *)copyOfOne
{
\tThing *base = [[Thing alloc] init];
\tThing *c = [base copy];
\t[base release];
\treturn c;
}
@end
";

fn run(cell: &Cell) {
        let src = format!(
                "{}{}\n#include <stdio.h>\nint main(void) {{\n{}\treturn 0;\n}}\n",
                ozobject_src(),
                DECLS,
                cell.body
        );
        /* A stem per cell, derived from `what`. Sharing one stem made
         * every row report the *first* row's output -- the harness keys
         * its build directory on the stem, so twelve cells collided on
         * one binary and the matrix agreed with itself about nothing.
         * That is this file's own instance of the trap #400 warns about:
         * a helper that manufactures a finding. */
        let stem: String = cell
                .what
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
        let stem = format!("som_{}", &stem[..stem.len().min(60)]);
        let got = if cell.needs_reflection {
                compile_and_run_with_reflection(&src, &stem)
        } else if cell.needs_heap {
                compile_and_run_with_heap(&src, &stem)
        } else {
                compile_and_run(&src, &stem)
        };
        match cell.known_defect {
                None => assert_eq!(got, cell.expect, "{}: wrong output", cell.what),
                Some(issue) => assert_eq!(
                        got, cell.expect,
                        "{}: this cell is a KNOWN DEFECT ({}) asserted to still \
                         misbehave, and it no longer does. If you fixed it, correct \
                         the expectation and drop the marker in the same change -- \
                         that is what keeps this list honest.",
                        cell.what, issue
                ),
        }
}

/* ---- the owning set: selectors that hand back +1 ------------------- */

#[test]
fn the_owning_set() {
        for cell in [
                Cell {
                        what: "+alloc / -init",
                        body: "\tThing *t = [[Thing alloc] init];\n\tprintf(\"ok %d\\n\", t != 0);\n",
                        expect: "ok 1\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "+new, declared by the class itself",
                        body: "\tThing *t = [Thing new];\n\tprintf(\"ok %d\\n\", t != 0);\n",
                        expect: "ok 1\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "+dynamicAlloc",
                        /* The heap counterpart of `+alloc`, and bound
                         * *bare* on purpose: `[[Thing dynamicAlloc] init]`
                         * is `+1` through the outer `-init` whatever the
                         * receiver's provenance was, so it would pass with
                         * the selector missing from
                         * `arc::CREATE_RULE_SELECTORS`. Verified red with
                         * it removed. `+dynamicAllocWithHeap:` needs an
                         * `OZHeap` instance this file's shared preamble
                         * does not carry, and is covered end to end in
                         * `behavior_foundation_heap.rs` instead. */
                        body: "\tThing *t = [Thing dynamicAlloc];\n\tprintf(\"ok %d\\n\", t != 0);\n",
                        expect: "ok 1\nd\n",
                        needs_reflection: false,
                        needs_heap: true,
                        known_defect: None,
                },
                Cell {
                        what: "-copy",
                        body: "\tThing *t = [[Thing alloc] init];\n\tThing *c = [t copy];\n\tprintf(\"ok %d\\n\", c != 0);\n",
                        expect: "ok 1\nd\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "-mutableCopy -- in the set, declared nowhere in the SDK, \
                               and sent by nothing in the tree before this row",
                        body: "\tThing *t = [[Thing alloc] init];\n\tThing *m = [t mutableCopy];\n\tprintf(\"ok %d\\n\", m != 0);\n",
                        expect: "ok 1\nd\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
        ] {
                run(&cell);
        }
}

/* ---- the consume set: sends that account for their receiver -------- */

#[test]
fn the_consume_set() {
        for cell in [
                Cell {
                        // The sharp edge in `released_by_hand`, and the reason
                        // this row exists: *one* manual release hands ARC the
                        // whole local. It does not subtract one reference and
                        // keep managing the rest -- it stops managing `t`
                        // entirely. So `retain` + `release` balance each other
                        // and the `alloc`'s reference is the author's, unreleased
                        // here, exactly as manual retain/release behaves without
                        // ARC. No dealloc, and that is the design rather than a
                        // defect: an author who took control keeps it.
                        what: "-retain by hand, balanced by a manual release -- ARC \
                               stops managing the local entirely",
                        // `live %d` is not decoration. This is the one cell
                        // that expects *no* dealloc, so without a non-null
                        // check a failed allocation would produce the same
                        // output and the cell would pass vacuously -- the
                        // fixture's `-copy` allocates, so a cell can exhaust
                        // a slab. Every other cell is protected by the
                        // dealloc it asserts: a `d` cannot print for an
                        // object that was never allocated.
                        body: "\tThing *t = [[Thing alloc] init];\n\tprintf(\"live %d\\n\", t != 0);\n\t[t retain];\n\t[t release];\n\tprintf(\"ok\\n\");\n",
                        expect: "live 1\nok\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "-release by hand over an ARC-managed local -- ARC defers \
                               to the author (`released_by_hand`)",
                        body: "\tThing *t = [[Thing alloc] init];\n\t[t release];\n\tprintf(\"ok\\n\");\n",
                        expect: "d\nok\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "-autorelease by hand -- implemented nowhere outside \
                               runtime_legacy, so a class must supply its own",
                        body: "\tThing *t = [[[Thing alloc] init] autorelease];\n\tprintf(\"ok %d\\n\", t != 0);\n",
                        expect: "ok 1\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "a selector that merely begins with `init` is not an \
                               initialiser (#398)",
                        body: "\tprintf(\"v=%d\\n\", [[Thing alloc] initialValue]);\n",
                        expect: "v=42\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
        ] {
                run(&cell);
        }
}

/* ---- @autoreleasepool ---------------------------------------------- */

#[test]
fn the_autoreleasepool_construct() {
        for cell in [
                Cell {
                        what: "@autoreleasepool is a scope: a +1 declared inside is \
                               released at its end, not the function's",
                        body: "\t@autoreleasepool {\n\t\tThing *t = [[Thing alloc] init];\n\t\tprintf(\"in %d\\n\", t != 0);\n\t}\n\tprintf(\"out\\n\");\n",
                        expect: "in 1\nd\nout\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "a +1 escaping the pool through an outer local is not \
                               released at the pool's end",
                        body: "\tThing *e = 0;\n\t@autoreleasepool {\n\t\te = [[Thing alloc] init];\n\t}\n\tprintf(\"alive %d\\n\", e != 0);\n\t[e release];\n",
                        expect: "alive 1\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
        ] {
                run(&cell);
        }
}

/* ---- the -performSelector: family ---------------------------------- */

#[test]
fn the_perform_selector_family() {
        for cell in [
                Cell {
                        what: "-performSelector: with a statically resolvable +1 \
                               selector, result bound to a typed local",
                        body: "\tThing *t = [[Thing alloc] init];\n\tThing *c = (Thing *)[t performSelector:@selector(copy)];\n\tprintf(\"ok %d\\n\", c != 0);\n",
                        expect: "ok 1\nd\nd\n",
                        needs_reflection: true,
                        needs_heap: false,
                        known_defect: None,
                },
        ] {
                run(&cell);
        }
}

/* ---- compositions, where the earlier audits found the worst cases -- */

#[test]
fn the_compositions() {
        for cell in [
                Cell {
                        what: "-copy of a +1: the receiver's reference is discarded, \
                               the copy is kept",
                        body: "\tThing *c = [[[Thing alloc] init] copy];\n\tprintf(\"ok %d\\n\", c != 0);\n",
                        expect: "d\nok 1\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "a factory returning a -copy",
                        body: "\tMaker *m = [[Maker alloc] init];\n\tThing *t = [m copyOfOne];\n\tprintf(\"ok %d\\n\", t != 0);\n",
                        expect: "d\nok 1\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "an `id`-typed local holding a +1 -- `id` carries no `*`, \
                               so the local was never managed and the reference leaked \
                               (#400)",
                        body: "\tThing *t = [[Thing alloc] init];\n\tid c = [t copy];\n\tprintf(\"ok %d\\n\", c != 0);\n",
                        expect: "ok 1\nd\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        what: "a `void *` local holding a +1",
                        body: "\tThing *t = [[Thing alloc] init];\n\tvoid *c = [t copy];\n\tprintf(\"ok %d\\n\", c != 0);\n",
                        expect: "ok 1\nd\nd\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
                Cell {
                        /* `-copy` is the one owning selector whose receiver
                         * can be the slot being stored into, and that is
                         * the composition #424 was: the store releases the
                         * previous value only for the right-hand sides it
                         * can evaluate *after* the release. A `static`
                         * local's store was not one of them, so the
                         * original was never released -- this row printed
                         * `ok 1` with no `d` before it.
                         *
                         * Counting could not have caught it either way:
                         * the `d` has to be observed, and it has to be
                         * observed *before* `ok 1`, which is what says the
                         * released pointer is the original and not the
                         * copy. */
                        what: "-copy into the static local it reads -- the original \
                               must be released by the store that replaces it (#424)",
                        body: "\tstatic Thing *cached;\n\n\tcached = [[Thing alloc] init];\n\tcached = [cached copy];\n\tprintf(\"ok %d\\n\", cached != 0);\n",
                        expect: "d\nok 1\n",
                        needs_reflection: false,
                        needs_heap: false,
                        known_defect: None,
                },
        ] {
                run(&cell);
        }
}
