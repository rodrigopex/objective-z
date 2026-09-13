// SPDX-License-Identifier: Apache-2.0
//
// managed_locals_are_body_scoped.rs -- a strong-local decision belongs to
// the body it was made in (#459).
//
// `EmitCtx::arc_managed_locals` and `arc_managed_slots` hold *names*, and
// `owned_locals_of` consults them by name alone. `collect_local_decls` used
// to `extend` them once per body and never clear, so a name left behind by
// a previously rendered body answered for a different body's local of the
// same name. Bodies render in source order, so an owning `t` in an earlier
// method released a **borrowed** `t` in a later one -- and writing the two
// methods the other way round was correct.
//
// That is the corrupting direction, not the leaking one: the reference
// freed belongs to the caller. Reproduced as an ASan `heap-use-after-free`
// from source `clang -fobjc-arc -Weverything` accepts with zero
// diagnostics, needing no cast, no attribute and no construct outside the
// ordinary subset.
//
// **Why this is not a row in `ownership_matrix.rs`.** Every row there names
// one generated function and counts the refcount traffic inside it
// (`Shape::func`). This defect cannot be expressed that way: it needs two
// methods *and* their relative order, and the wrong answer appears in the
// function that is correct in isolation. So it gets its own file, and the
// matrix's own contract -- "everything not listed is believed correct" --
// is why saying that out loud matters rather than quietly adding a row that
// could not fail.
//
// Each test asserts **both** orderings. Asserting only the broken one would
// pass on a fix that broke the other, and the order dependence is the
// clearest evidence that the decision was leaking across bodies at all.
//
// The blast radius is the *method* arm only. Every method in an
// `@implementation` renders through one shared `EmitCtx`; a plain C
// function builds its own, so its set starts empty. That was measured, not
// assumed -- the free-function test below was written expecting to fail
// without the fix and passes either way.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// Shared declarations: a class that counts its own teardown, so the
/// oracle is an observed side effect rather than a refcount count.
///
/// Counting releases in the emitted text would *also* catch this, and less
/// sharply: it cannot say which pointer a release names, which is what #398
/// got wrong. A `-dealloc` that increments a counter says whether the
/// object actually died.
const DECLS: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag {
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end
";

/// The defect's own shape: a method that *owns* a local named `t`, written
/// **before** a method that merely *borrows* one.
///
/// `-borrow:` must release nothing -- `arg` is the caller's. Before the fix
/// it emitted `oz_static_release(t)` and destroyed a live object one call
/// in, which the dealloc count sees as `1` where it must be `0`.
#[test]
fn a_borrowed_local_survives_an_owning_namesake_declared_earlier() {
    let src = format!(
        "/* oz-pool: Thing=2,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (void)make;
- (int)borrow:(Thing *)arg;
@end
@implementation P
/* Owns its `t`; its release is correct and must stay. */
- (void)make {
	Thing *t = [[Thing alloc] init];
	(void)[t tag];
}
/* Borrows. The only thing it shares with -make is the name of the local. */
- (int)borrow:(Thing *)arg {
	Thing *t = arg;
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	Thing *owned = [[Thing alloc] init];
	int first = [p borrow:owned];
	int deallocs_after_borrow = g_deallocs;
	int second = [p borrow:owned];
	printf(\"first=%d second=%d after_borrow=%d\\n\",
	       first, second, deallocs_after_borrow);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "borrowed_local_survives_earlier_namesake");
    /* `owned` is still live at both calls, so neither may tear it down.
     * `after_borrow=1` is the defect: the first -borrow: freed it, and the
     * second read and released freed memory. */
    assert_eq!(
        out, "first=7 second=7 after_borrow=0\n",
        "-borrow: released a reference it does not own: {}",
        out
    );
}

/// The same two methods the other way round, which was **already correct**
/// before the fix and must stay correct after it.
///
/// This is the half that makes the pair a real test. A fix that scoped the
/// set too aggressively -- or reset it in the wrong place -- would break
/// `-make`'s own release here while leaving the test above green.
#[test]
fn an_owning_local_still_releases_with_a_borrowing_namesake_declared_earlier() {
    let src = format!(
        "/* oz-pool: Thing=2,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)borrow:(Thing *)arg;
- (void)make;
@end
@implementation P
- (int)borrow:(Thing *)arg {
	Thing *t = arg;
	return [t tag];
}
- (void)make {
	Thing *t = [[Thing alloc] init];
	(void)[t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	Thing *owned = [[Thing alloc] init];
	int borrowed = [p borrow:owned];
	int after_borrow = g_deallocs;
	[p make];
	printf(\"borrowed=%d after_borrow=%d after_make=%d\\n\",
	       borrowed, after_borrow, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_local_releases_after_borrowing_namesake");
    /* -make allocates and must tear its own instance down; -borrow: must
     * still leave the caller's alone. So exactly one dealloc, and it
     * happens in -make. */
    assert_eq!(
        out, "borrowed=7 after_borrow=0 after_make=1\n",
        "-make must release its own instance and only its own: {}",
        out
    );
}

/// A plain C function's body is the second top-level ARC scope, and it was
/// **never** affected -- which is worth a test saying so.
///
/// Measured rather than assumed, and the assumption was wrong: I expected
/// this to fail without the fix and it passes either way. A free function
/// builds its own `EmitCtx::new(...)` per function, so its
/// `arc_managed_locals` starts empty, while every method in an
/// `@implementation` shares one context -- which is what confines #459 to
/// the method arm.
///
/// So this is a control, not a regression case. It is kept because the
/// containment is the reason the blast radius is small, and nothing else
/// records it: if free functions are ever moved onto the shared context --
/// a reasonable-looking simplification -- this test is what notices that
/// they have joined the contaminated set.
#[test]
fn a_borrowed_local_in_a_plain_c_function_survives_a_method_namesake() {
    let src = format!(
        "/* oz-pool: Thing=2,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (void)make;
@end
@implementation P
- (void)make {
	Thing *t = [[Thing alloc] init];
	(void)[t tag];
}
@end

/* A plain C function, rendered after the @implementation above, whose
   local is borrowed and happens to be called `t` too. */
static int borrow_in_c(Thing *arg)
{
	Thing *t = arg;
	return [t tag];
}

#include <stdio.h>
int main(void) {
	Thing *owned = [[Thing alloc] init];
	int first = borrow_in_c(owned);
	int after = g_deallocs;
	int second = borrow_in_c(owned);
	printf(\"first=%d second=%d after=%d\\n\", first, second, after);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "borrowed_local_in_c_function_survives_namesake");
    assert_eq!(
        out, "first=7 second=7 after=0\n",
        "a plain C function released a reference it does not own: {}",
        out
    );
}

/// The emitted text, so a failure says *where* rather than only that a
/// count was wrong.
///
/// The runtime tests above are the real oracle; this one exists because a
/// dealloc count of 1 where 0 was expected does not say which function
/// emitted the release, and that is the first thing anyone debugging this
/// will want.
#[test]
fn the_borrowing_method_emits_no_release() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (void)make;
- (int)borrow:(Thing *)arg;
@end
@implementation P
- (void)make {
	Thing *t = [[Thing alloc] init];
	(void)[t tag];
}
- (int)borrow:(Thing *)arg {
	Thing *t = arg;
	return [t tag];
}
@end
"
    );
    let out = oz_static::transpile(&src).expect("this source is inside the static subset");
    let c = out.source_c;

    let borrow = extract_fn(&c, "int P_borrow_");
    assert!(
        !borrow.contains("oz_static_release"),
        "P_borrow_ borrows its argument and must emit no release:\n{}",
        borrow
    );

    let make = extract_fn(&c, "void P_make");
    assert_eq!(
        make.matches("oz_static_release").count(),
        1,
        "P_make owns its local and must still release it exactly once:\n{}",
        make
    );
}

/// One generated function's text, by brace matching, skipping prototypes.
///
/// Both halves of that are lessons `docs/STATUS.md` already records, and I
/// walked into the second one writing this file.
///
/// Not "up to the first `}` in column zero": that is the broken extractor
/// which stopped inside any function containing a nested block and made
/// `@synchronized` on an owned local look like a leak.
///
/// And not the *first* occurrence of the signature either. The generated
/// `.c` opens with a prototype block, so `P_borrow_` appears first as
/// `int P_borrow_(struct P *self, struct Thing * arg);` -- brace matching
/// from there skips past the `;` and returns the **next** function's body,
/// which is `P_make`, which legitimately contains a release. The first
/// draft of this file reported the bug as unfixed on that basis while all
/// three runtime tests passed. A false finding from a broken instrument
/// reads exactly like a real one.
fn extract_fn(c: &str, signature: &str) -> String {
    /* The definition is the occurrence whose next punctuation is `{`; a
     * prototype's is `;`. */
    let start = c
        .match_indices(signature)
        .find(|(at, _)| {
            c[*at..]
                .find(|ch| ch == '{' || ch == ';')
                .is_some_and(|k| c[*at..].as_bytes()[k] == b'{')
        })
        .map(|(at, _)| at)
        .unwrap_or_else(|| {
            panic!("no *definition* of {:?} in the generated C:\n{}", signature, c)
        });
    let rest = &c[start..];
    let mut depth = 0usize;
    let mut seen_open = false;
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => {
                depth += 1;
                seen_open = true;
            }
            '}' => {
                depth -= 1;
                if seen_open && depth == 0 {
                    return rest[..=i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after {:?}", signature)
}
