// SPDX-License-Identifier: Apache-2.0
//
// absence_claims_need_a_parse.rs -- a check that asserts something is
// *absent* is only sound over a tree that parsed (#566/#567 regression).
//
// #576 added two absence claims:
//
//   #567  "no `@interface` for this name exists in this source"
//   #566  "this selector is declared and defined nowhere"
//
// Both are wrong on malformed input, because a syntax error does not make
// the class graph merely incomplete -- it makes it *misleading*:
//
//   @interface : OZObject      a nameless interface (M21). tree-sitter puts
//                              an ERROR on the stray `:` and leaves
//                              `OZObject` as the first `identifier`, so
//                              `class_header` reads the *superclass* as the
//                              class name and the real class is declared
//                              nowhere.
//
//   int x = [self value;       an unclosed bracket (M04/M05/M09). The body
//                              stops parsing, so a method that *is* defined
//                              looks undefined.
//
// In each case the conclusion is true of the parse, derived from the syntax
// error, and points at a construct that is not the mistake -- #567's
// diagnostic named the `@implementation` while the error was on the
// `@interface` line above it.
//
// `oz2c-challenges` grades all four fixtures `CLANG`: the C front end is
// the right reporter, with `expected identifier` and `expected ']'`. #567's
// check made that unreachable, because it runs in `collect`, which hard-
// gates and returns *before* `attach_ast`. #566's was saved only by the AST
// gate firing first -- true in a real build, false under
// `--allow-missing-ast`, so luck rather than design.
//
// Found by sweeping the whole 99-fixture corpus against the pre-#576
// binary, which is the check that should have run before #576 was filed
// rather than four PRs later: asking "which fixtures does my new check
// newly reach?" is a different question from "is the fixture the issue
// names graded correctly?".

mod common;
use common::{expect_reject, ozobject_src as PREAMBLE};

/// A nameless `@interface` must not be reported as a missing one.
#[test]
fn a_nameless_interface_is_not_reported_as_a_missing_interface() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface : OZObject {
	int _value;
}
- (int)run;
@end

@implementation Probe
- (int)run
{
	return 21;
}
@end
"
    );
    /* **Deferred, not refused**, and that is the contract rather than a
     * concession. `oz2c-challenges` grades M21 `CLANG`: the C front end is
     * the right reporter, and it says `expected identifier` -- which names
     * the `@interface` line, where the mistake is. It is also what these
     * fixtures did before #576, so this restores a behaviour the corpus
     * endorses rather than inventing one.
     *
     * In a real build the AST requirement refuses first (Clang cannot
     * produce a dump for source it cannot parse), so nothing reaches the C
     * compiler unchecked. This is the `--allow-missing-ast` path, where the
     * caller has said to trust them about the AST. */
    let diags = match oz2c::transpile(&src) {
        Ok(_) => String::new(),
        Err(d) => d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"),
    };
    assert!(
        !diags.contains("has no '@interface Probe'"),
        "#567's absence claim fired over a tree that did not parse:\n{}",
        diags
    );
}

/// An unclosed bracket must not be reported as an undefined method.
#[test]
fn an_unclosed_bracket_is_not_reported_as_an_undefined_method() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)value;
- (int)run;
@end

@implementation Probe
- (int)value { return 4; }
- (int)run
{
	int x = [self value;

	return x;
}
@end
"
    );
    /* Deferred for the same reason -- M04/M05/M09 are graded `CLANG`, and
     * Clang says `expected ']'`, which names the bracket. */
    let diags = match oz2c::transpile(&src) {
        Ok(_) => String::new(),
        Err(d) => d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"),
    };
    assert!(
        !diags.contains("defined nowhere"),
        "#566's absence claim fired over a tree that did not parse:\n{}",
        diags
    );
}

/* ------------------------------------------------------------------------
 * The presence half. Gating on a clean parse must not become a blanket
 * "stop checking on bad input": a construct that is really there is really
 * there whatever else failed. Without these, the fix above could be
 * implemented as `if !source_parsed { return Ok(()) }` over the whole front
 * end and all the tests would still pass.
 * --------------------------------------------------------------------- */

/// #567's real case, over source that parses. Unchanged.
#[test]
fn a_genuinely_missing_interface_is_still_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "@implementation Lonely\n- (int)answer { return 42; }\n@end\n"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("'@implementation Lonely' has no '@interface Lonely' in this source"),
        "{}",
        diags
    );
}

/// #566's real case, over source that parses. Unchanged.
#[test]
fn a_genuinely_undefined_method_is_still_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)missing;
- (int)run;
@end

@implementation Probe
- (int)run
{
	return [self missing];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'-missing' is declared on 'Probe' and defined nowhere"), "{}", diags);
}

/// And a refusal that is a **presence** claim still fires on malformed
/// source, which is the boundary this fix must not cross. `@throw` is
/// really there whatever else failed to parse (#563).
#[test]
fn a_presence_claim_still_fires_over_a_broken_parse() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	@throw nil;
	int x = [self value;

	return x;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("'@throw' is not in the static subset"),
        "a presence claim was lost to the parse gate:\n{}",
        diags
    );
}
