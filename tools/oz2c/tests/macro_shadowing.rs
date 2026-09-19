// SPDX-License-Identifier: Apache-2.0
//
// macro_shadowing.rs -- a source `#define` must not be able to rewrite
// oz2c's own output (#571).
//
// oz2c copies a file-scope `#define` into the generated header verbatim --
// deliberately, since a macro may be a constant the emitted C needs -- and
// emits its own identifiers from the raw source token. When the two
// namespaces collide the copied macro rewrites *some* occurrences and not
// the rest, and the generated C stops type-checking on lines the author
// never wrote.
//
// What makes the reported case fail is the interaction with oz2c's own
// include order, not the copy alone:
//
//     #include "oz2c_dispatch.h"   <- declares Alias_run(struct Alias *)
//     #define Alias Probe          <- copied from source, AFTER that
//     struct Alias { ... };        <- now reads `struct Probe`
//     int Alias_run(struct Alias *self);      <- and so does this
//
// A function *name* is one token the preprocessor cannot reach into, so
// both declarations are `Alias_run` while their parameter types differ.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// A class whose methods, slab and synthesized members are all present, so
/// the generated output carries every per-class identifier shape. Shared by
/// the refusal cases and by the drift guard below.
fn probe_src(extra: &str) -> String {
    format!(
        "{}{}{}",
        PREAMBLE(),
        extra,
        "\
@interface Probe : OZObject {
	int _n;
}
+ (int)tally;
- (int)run;
- (void)dealloc;
@end

@implementation Probe
+ (int)tally { return 7; }
- (int)run { return 93; }
- (void)dealloc { _n = 0; }
@end

int drive(void)
{
	Probe *p = [Probe alloc];

	return [p run];
}
"
    )
}

/// The reported shape: the class name itself, spelled through a macro.
#[test]
fn a_macro_named_after_a_class_is_refused() {
    let diags = expect_reject(&probe_src("#define Probe Other\n\n"));
    assert!(
        diags.contains("macro 'Probe' has the same name as an identifier the generated C emits"),
        "{}",
        diags
    );
    /* The note has to explain why only *some* occurrences move, or the
     * author cannot act on it: `struct Probe` is two tokens and
     * `Probe_run` is one. */
    assert!(diags.contains("two tokens"), "{}", diags);
    assert!(diags.contains("spell the class by its own name"), "{}", diags);
}

/// A synthesized member, which takes the other branch of the note.
#[test]
fn a_macro_named_after_a_synthesized_member_is_refused() {
    let diags = expect_reject(&probe_src("#define Probe_oz_alloc something\n\n"));
    assert!(
        diags.contains("macro 'Probe_oz_alloc' has the same name"),
        "{}",
        diags
    );
    assert!(diags.contains("oz2c synthesizes"), "{}", diags);
}

/// A method's mangled name, through the same mangler emit uses -- so the
/// `_cls` suffix on a class method is covered too.
#[test]
fn a_macro_named_after_a_class_method_symbol_is_refused() {
    let diags = expect_reject(&probe_src("#define Probe_tally_cls something\n\n"));
    assert!(diags.contains("macro 'Probe_tally_cls' has the same name"), "{}", diags);
}

/// A function-like macro of a colliding name is refused too -- the copy is
/// verbatim either way, and `preproc_function_def` reaches the same check.
#[test]
fn a_function_like_macro_of_a_colliding_name_is_refused() {
    let diags = expect_reject(&probe_src("#define Probe_run(x) ((x) + 1)\n\n"));
    assert!(diags.contains("macro 'Probe_run' has the same name"), "{}", diags);
}

/* ------------------------------------------------------ accepting cases */

/// **Accepting, and the reason this is a set rather than a prefix test.**
///
/// `Probe_MAX` shares the `Probe_` prefix and collides with nothing,
/// because the generated C never emits that name. A prefix rule would
/// reject ordinary source to catch an exotic case, so the check is keyed on
/// the exact set of identifiers actually emitted.
#[test]
fn a_macro_merely_prefixed_by_a_class_name_is_accepted() {
    let src = format!(
        "{}{}",
        probe_src("#define Probe_MAX 32\n\n"),
        "\
#include <stdio.h>
int main(void) {
	printf(\"%d %d\\n\", drive(), Probe_MAX);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "macro_prefixed_by_class"), "93 32\n");
}

/// **Accepting.** An ordinary constant macro still reaches the generated C,
/// which is why the copy exists at all and why dropping every `#define`
/// would be the wrong fix.
#[test]
fn an_unrelated_macro_still_reaches_the_generated_c() {
    let src = format!(
        "{}{}",
        probe_src("#define ANSWER 42\n\n"),
        "\
#include <stdio.h>
int main(void) {
	printf(\"%d %d\\n\", drive(), ANSWER);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "unrelated_macro_survives"), "93 42\n");
}

/* ------------------------------------------------------- the drift guard */

/// Every `Probe`-related identifier the generated C actually emits must be
/// one the check refuses a macro for.
///
/// This is a guard against **drift, not against a mistake**. The emitted
/// spellings live in `companion.rs` as `format!` strings; a new synthesized
/// member added there would not appear in
/// `staticbar::emitted_class_identifiers`, and the set would silently stop
/// covering the thing it exists to cover -- with every test above still
/// green, because they each name a shape that was already handled.
///
/// So this reads the identifiers out of a real transpile's **output** and
/// drives the check with each one, rather than asserting anything about the
/// set directly. It fails when `companion.rs` grows a shape, which is the
/// moment someone needs to know.
#[test]
fn the_emitted_identifier_set_still_covers_the_output() {
    let out = oz2c::transpile(&probe_src("")).expect("the probe program is inside the subset");
    let all = format!("{}\n{}\n{}", out.source_c, out.companion_h, out.companion_c);

    /* Identifiers that are `Probe`, `Probe_...`, or `oz_slab_Probe` --
     * every per-class shape `companion.rs` emits is one of those three. */
    let mut found: Vec<String> = Vec::new();
    for raw in all.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        let is_ours = raw == "Probe"
            || raw.starts_with("Probe_")
            || raw == "oz_slab_Probe";
        if is_ours && !found.contains(&raw.to_string()) {
            found.push(raw.to_string());
        }
    }

    /* A cell that fails when nothing ran: an empty sweep would otherwise
     * pass this test while checking nothing at all. */
    assert!(
        found.len() >= 4,
        "the sweep found only {:?} -- it is not reading the generated output any more",
        found
    );

    let mut uncovered: Vec<String> = Vec::new();
    for name in &found {
        let probe = probe_src(&format!("#define {} something\n\n", name));
        match oz2c::transpile(&probe) {
            Err(diags) => {
                let text: String =
                    diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
                if !text.contains("the same name as an identifier the generated C emits") {
                    uncovered.push(format!("{} (refused for another reason)", name));
                }
            }
            Ok(_) => uncovered.push(name.clone()),
        }
    }
    assert!(
        uncovered.is_empty(),
        "these identifiers are emitted but a macro shadowing them is not refused -- \
         `staticbar::emitted_class_identifiers` has drifted from `companion.rs`: {:?}\n\
         (all {} swept: {:?})",
        uncovered,
        found.len(),
        found
    );
}
