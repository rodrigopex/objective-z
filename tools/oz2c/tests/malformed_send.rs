// SPDX-License-Identifier: Apache-2.0
//
// malformed_send.rs -- a send that is not a send is refused with a
// location, rather than panicking or quietly losing a token (#494).
//
// `emit::parse_message` returned `MessageParts` unconditionally, so it had
// no way to say "this is not a well-formed send" and its ten callers across
// five modules were all written against the assumption that it is. Its loop
// advanced `i` by three while its guard tested `i + 1`, and a missing colon
// then did one of two things:
//
//   * `[s isEqual other]` -- three children, the guard `2 < 3` passes,
//     `children[3]` **panics**: `index out of bounds: the len is 3 but the
//     index is 3`. It came from `collect::prescan_reflection`, before any
//     check could run, and nothing was written to the output directory.
//
//   * `[s take:n n]` -- five children, one iteration runs and the guard
//     then stops it, so the trailing token is **silently dropped** and
//     `T_take_(self, n)` is emitted. Measured on the unfixed tree: that
//     generated C compiles with **zero** errors. Nothing downstream catches
//     it, which makes it worse than the panic -- a panic at least stops the
//     build.
//
// Clang rejects both with `expected ':'` and a column. That is not a
// backstop this can lean on: the panic reproduced on a path that *did*
// produce an AST dump, which is how it was found.
//
// The length test alone is not enough, and that is the subtle half.
// `[a b c d]` is four children, satisfies `(len - 1) % 3 == 0`, and is
// still malformed -- it was the shape that emitted a call to a selector the
// author never wrote. So well-formedness also requires a literal `:` at
// every `2 + 3k`, confirmed against the parse:
//
//     [a b]        len 2   receiver, selector
//     [a b:1]      len 4   receiver, keyword, ':', argument
//     [a b:1 c:2]  len 7   receiver, then three children per keyword
//
// What is **not** fixed here, and why: `[]` parses as a tree-sitter `ERROR`
// node rather than a `message_expression`, so it never reaches
// `parse_message`. It makes oz2c exit 0 with raw Objective-C in the `.c`.
// Refusing `ERROR` nodes generally is not available: `samples/zbus_service`
// -- which the twister sweep builds in 15 configurations -- contains two of
// them, from bare Zephyr macros at file scope that
// `parse::repair_bare_macro_statements` does not normalise. Measured across
// 202 repo-owned sources: four contain `ERROR` nodes, two of them in that
// live sample. So the fourth defect needs a narrower rule and its own
// change.

mod common;
use common::{expect_reject, ozobject_src};

fn program(body: &str) -> String {
    format!(
        "{}{}",
        ozobject_src(),
        format!(
            "\
@interface T : OZObject
- (void)take:(int)x;
- (void)two:(int)x and:(int)y;
- (void)poke;
@end
@implementation T
- (void)take:(int)x
{{
	(void)x;
}}
- (void)two:(int)x and:(int)y
{{
	(void)x;
	(void)y;
}}
- (void)poke
{{
	int n = 1;

	(void)n;
	{}
}}
@end

int main(void) {{ return 0; }}
",
            body
        )
    )
}

/// Every malformed shape, refused with a location in the author's own `.m`.
///
/// The location is the point. Two of these used to panic with no output at
/// all, and two used to produce C -- in one case C that compiles. What the
/// author could not get from any of them was a line to look at.
#[test]
fn every_malformed_send_is_refused_with_a_location() {
    let cases: &[(&str, &str)] = &[
        /* Three children: the panic #494 was filed for. */
        ("no colon", "[self take n];"),
        /* Three children, the colon parsed as an ERROR node. */
        ("trailing colon", "[self take:];"),
        /* Four children -- passes the arithmetic length test, which is why
         * the colon positions have to be checked too. Emitted
         * `T_take_(self, more)`: a selector the source never wrote, with
         * the first operand dropped. */
        ("four pieces, no colons", "[self take n more];"),
        /* Five children: one valid keyword then a stray token. Emitted
         * `T_take_(self, n)` and compiled clean. */
        ("valid keyword then a stray token", "[self take:n n];"),
    ];
    for (name, body) in cases {
        let diags = expect_reject(&program(body));
        assert!(
            diags.contains("is not a well-formed message send"),
            "the '{}' shape must be refused by oz2c's own diagnostic; got:\n{}",
            name,
            diags
        );
        assert!(
            diags.contains("needs a ':'"),
            "and must say what is missing, the way Clang's `expected ':'` does; got:\n{}",
            diags
        );
        assert!(
            /* `LINE:COL: message`, read from the left -- the message
             * itself contains colons, so splitting from the right finds
             * prose rather than the position. */
            diags.lines().any(|l| {
                let mut parts = l.trim_start().splitn(3, ':');
                let line = parts.next().unwrap_or("");
                let col = parts.next().unwrap_or("");
                !line.is_empty()
                    && line.chars().all(|c| c.is_ascii_digit())
                    && !col.is_empty()
                    && col.chars().all(|c| c.is_ascii_digit())
            }),
            "and must carry a line:column -- the panic gave none, and the two silent shapes \
             gave none either; got:\n{}",
            diags
        );
    }
}

/// The well-formed shapes are still accepted.
///
/// The risk of a stricter predicate is over-refusal, and these are what
/// would catch it: a unary send, one keyword, two keywords, a nested send
/// as a receiver, and a send as an argument. Every one of them goes through
/// the same `parse_message` the malformed cases do.
#[test]
fn well_formed_sends_are_still_accepted() {
    let cases: &[(&str, &str)] = &[
        ("unary", "[self poke];"),
        ("one keyword", "[self take:n];"),
        ("two keywords", "[self two:n and:n];"),
        ("nested receiver", "[[T alloc] poke];"),
        ("send as argument", "[self take:[self dummy]];"),
    ];
    for (name, body) in cases {
        /* `dummy` needs declaring for the last row; the others ignore it. */
        let src = program(body).replace(
            "- (void)poke;\n@end",
            "- (void)poke;\n- (int)dummy;\n@end",
        )
        .replace(
            "- (void)poke\n{",
            "- (int)dummy\n{\n\treturn 0;\n}\n- (void)poke\n{",
        );
        assert!(
            oz2c::transpile(&src).is_ok(),
            "the '{}' shape is well formed and must still be accepted -- a stricter \
             well-formedness test over-refusing is the risk this case exists to catch; got:\n{}",
            name,
            oz2c::transpile(&src)
                .err()
                .map(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default()
        );
    }
}

/// The silent shape is refused **rather than emitting the call it used to**.
///
/// Its own case, because "refused" is not by itself evidence that the right
/// thing happened: the two panicking shapes would satisfy a refusal
/// assertion the moment they stopped panicking, whatever else changed. What
/// makes this one specific is the emission that must no longer exist.
/// `[self take:n n]` used to produce `T_take_((struct T *)(self), n)` -- a
/// one-argument call from a source Clang rejects -- and that C compiled with
/// zero errors, so no gate downstream of oz2c could have caught it.
#[test]
fn the_silently_dropped_token_no_longer_emits_a_call() {
    let src = program("[self take:n n];");
    match oz2c::transpile(&src) {
        Ok(out) => panic!(
            "expected a refusal; instead it transpiled, and the emitted C contains \
             T_take_: {}",
            out.source_c.lines().filter(|l| l.contains("T_take_")).collect::<Vec<_>>().join(" | ")
        ),
        Err(diags) => {
            let text = diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
            assert!(
                text.contains("is not a well-formed message send"),
                "and refused for being malformed rather than for some later reason -- \
                 otherwise this case would pass on a tree where the token is still \
                 dropped; got:\n{}",
                text
            );
        }
    }
}

/// One diagnostic per malformed send, not one per token inside it.
///
/// A malformed send's children are not a reliable shape, so descending into
/// it to look for a nested send would report positions the author cannot act
/// on until the outer send is fixed. Two malformed sends in one body are two
/// mistakes and get two diagnostics; one malformed send gets one.
#[test]
fn each_malformed_send_is_reported_once() {
    let one = expect_reject(&program("[self take n];"));
    assert_eq!(
        one.matches("is not a well-formed message send").count(),
        1,
        "one malformed send, one diagnostic; got:\n{}",
        one
    );

    let two = expect_reject(&program("[self take n];\n\t[self take m];"));
    assert_eq!(
        two.matches("is not a well-formed message send").count(),
        2,
        "two malformed sends are two mistakes to fix; got:\n{}",
        two
    );
}
