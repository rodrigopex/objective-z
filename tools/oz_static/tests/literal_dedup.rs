// literal_dedup.rs -- identical boxed string literals share one instance
// within an origin (#372).
//
// The point is not the bytes. Objective-C guarantees that identical
// literals in a translation unit are the same object, and `-isEqual:`
// opens with `if (self == anObject) { return YES; }`
// (`src/OZString.m`). With one instance per *occurrence* that branch
// missed for two spellings of the same string, so `@"a" == @"a"` was
// false where the language says it is true. Measured across every sample
// in the tree the footprint saving is 3 instances, 72 bytes of `.rodata`;
// px-keyboard, the only real application, contains no boxed literal at
// all. So these tests assert identity, and the size assertion is the
// weakest of them.

mod common;

use common::{compile_and_run, ozobject_src, ozstring_src};

fn program(body: &str) -> String {
        format!(
                "{}{}\n#include <stdio.h>\n{}",
                ozobject_src(),
                ozstring_src(),
                body
        )
}

/// The behaviour the language promises, and the reason for the change.
///
/// Without dedup this reports `same=0`: two occurrences, two instances,
/// two addresses. `eq` was always 1 -- `-isEqual:` falls through to
/// `_length` and `memcmp` -- which is exactly why the divergence was
/// invisible to every test that asked about equality rather than
/// identity.
#[test]
fn identical_literals_are_the_same_object() {
        let src = program(
                "\
int main(void) {
\tOZString *a = @\"hello\";
\tOZString *b = @\"hello\";
\tOZString *c = @\"other\";
\tprintf(\"same=%d\\n\", a == b);
\tprintf(\"eq=%d\\n\", [a isEqual:b]);
\tprintf(\"distinct=%d\\n\", a == c);
\tprintf(\"still=%s|%s\\n\", [a cString], [c cString]);
\treturn 0;
}
",
        );
        let out = compile_and_run(&src, "identical_literals_are_the_same_object");
        assert_eq!(out, "same=1\neq=1\ndistinct=0\nstill=hello|other\n");
}

/// One definition per distinct string, not per occurrence.
#[test]
fn one_instance_is_emitted_per_distinct_string() {
        let src = program(
                "\
int main(void) {
\tOZString *a = @\"dup\";
\tOZString *b = @\"dup\";
\tOZString *c = @\"dup\";
\tOZString *d = @\"solo\";
\treturn [a length] + [b length] + [c length] + [d length];
}
",
        );
        let out = oz_static::transpile(&src).expect("should transpile").source_c;
        let defs: Vec<&str> = out
                .lines()
                .filter(|l| l.starts_with("const struct OZString _oz_str_"))
                .collect();
        assert_eq!(
                defs.len(),
                2,
                "three occurrences of one string plus one other must emit two \
                 instances; got {}:\n{}",
                defs.len(),
                defs.join("\n")
        );
        let protos = out
                .lines()
                .filter(|l| l.starts_with("extern const struct OZString _oz_str_"))
                .count();
        assert_eq!(
                protos, 2,
                "a forward declaration must survive for each definition and no \
                 others, or the .c refers to a symbol it never defines"
        );
}

/// The rename has to reach a hoisted block's own text, not just the
/// method bodies.
///
/// A block literal is emitted as a separate function whose body carries
/// its own copy of the expression text, so a literal used inside one is
/// referenced from a different bucket than the enclosing method's. If the
/// dedup pass renamed only `bodies`, this program would reference a
/// definition that had been dropped.
///
/// Confirmed to bite, by excluding the block and static buckets from the
/// rename and re-running: the generated C then says `use of undeclared
/// identifier '_oz_str_L431_C21_3'`. That is a *compile* error rather
/// than a link error, because a dropped literal's forward declaration
/// goes with its definition -- either way, reading the emitted text
/// would not have found it and compiling it does.
///
/// This test passes with the dedup pass absent entirely, which is
/// correct: with nothing renamed there is no dangling reference. It
/// guards the pass's completeness, not its existence.
#[test]
fn a_literal_inside_a_block_follows_the_rename() {
        let src = program(
                "\
int main(void) {
\tOZString *outer = @\"shared\";
\tint (^len)(void) = ^int(void) {
\t\tOZString *inner = @\"shared\";
\t\treturn (int)[inner length];
\t};
\tprintf(\"outer=%d inner=%d\\n\", (int)[outer length], len());
\treturn 0;
}
",
        );
        let out = compile_and_run(&src, "a_literal_inside_a_block_follows_the_rename");
        assert_eq!(out, "outer=6 inner=6\n");
}

/// Strings that differ only in the ways the key must notice.
///
/// The dedup key is the emitted definition with its own symbol name
/// removed, so it carries `._length` and `._data` both. These three are
/// pairwise distinct and must stay so: a prefix, the same bytes with a
/// trailing space, and the empty string.
#[test]
fn strings_that_differ_are_not_merged() {
        let src = program(
                "\
int main(void) {
\tOZString *a = @\"ab\";
\tOZString *b = @\"abc\";
\tOZString *c = @\"ab \";
\tOZString *d = @\"\";
\tprintf(\"%d %d %d %d\\n\", a == b, a == c, a == d, b == c);
\tprintf(\"%zu %zu %zu %zu\\n\", [a length], [b length], [c length], [d length]);
\treturn 0;
}
",
        );
        let out = compile_and_run(&src, "strings_that_differ_are_not_merged");
        assert_eq!(out, "0 0 0 0\n2 3 3 0\n");
}
