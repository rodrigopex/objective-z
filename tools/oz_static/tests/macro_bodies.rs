// SPDX-License-Identifier: Apache-2.0
//
// macro_bodies.rs - Objective-C inside a `#define` body is rejected (#238),
// while Objective-C passed as a macro *argument* is transpiled and the
// invocation preserved.
//
// The two halves belong in one file because the distinction is the whole
// point: a macro body is one opaque `preproc_arg` token that no walk
// descends into, while an argument is a real `message_expression` inside a
// `call_expression` that the ordinary expression renderer reaches. Testing
// only the rejection would leave nothing pinning the shape that works, and a
// future change to the walk could start expanding macros without failing a
// test.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// A class with one method to send, shared by the cases below.
///
/// `-run` returns an `int` and `main` prints it, so an accepted case is
/// checked by what it computed rather than by having compiled -- the
/// distinction this whole file rests on.
fn counter_src(extra_defines: &str, body: &str) -> String {
    format!(
        "{}\n{}\n@interface Counter : OZObject {{\n\tint _n;\n}}\n\
         - (int)value;\n- (void)bump;\n- (int)run;\n@end\n\
         @implementation Counter\n\
         - (int)value {{\n\treturn _n;\n}}\n\
         - (void)bump {{\n\t_n = _n + 1;\n}}\n\
         - (int)run {{\n{}\n}}\n@end\n\
         \n#include <stdio.h>\nint main(void) {{\n\
         \tCounter *c = [Counter alloc];\n\
         \tprintf(\"run=%d\\n\", [c run]);\n\
         \treturn 0;\n}}\n",
        PREAMBLE(),
        extra_defines,
        body
    )
}

// ---------------------------------------------------------------------------
// Rejected: Objective-C in a macro body
// ---------------------------------------------------------------------------

/// The shape #238 was filed on. Before the fix `oz2c` exited 0 and the
/// `#define` landed in the generated header verbatim, so the C compiler
/// failed on `GREET_VIA_BODY(c)` -- generated code the user never wrote,
/// with no oz_static diagnostic pointing at the `#define`.
#[test]
fn message_send_in_macro_body_rejected() {
    let src = counter_src("#define BUMP_VIA_BODY(obj) [obj bump]", "\tBUMP_VIA_BODY(self);\n\treturn 0;");
    let diags = expect_reject(&src);
    assert!(diags.contains("BUMP_VIA_BODY"), "diagnostics: {}", diags);
    assert!(diags.contains("message_expression"), "diagnostics: {}", diags);
}

/// The diagnostic must name the `#define`, since that is the thing to change.
/// Its location is the `#define` too, not a position inside the probe parse:
/// probe coordinates are offsets into a string the checker invented and mean
/// nothing to a reader.
#[test]
fn diagnostic_points_at_the_define_and_suggests_the_argument_form() {
    let src = counter_src("#define BUMP_VIA_BODY(obj) [obj bump]", "\tBUMP_VIA_BODY(self);\n\treturn 0;");
    let diags = expect_reject(&src);
    assert!(diags.contains("macro 'BUMP_VIA_BODY'"), "diagnostics: {}", diags);
    assert!(diags.contains("argument"), "diagnostics: {}", diags);
}

/// A body wrapping statements in `do { ... } while (0)` -- an ordinary macro
/// idiom, and the reason the probe uses a *statement* wrapper. The detector
/// prototyped on #238 wrapped the body as an expression, which cannot parse
/// this at all, so it would have been silently accepted by the
/// "does not parse cleanly, do not flag" guard.
#[test]
fn message_send_in_do_while_macro_body_rejected() {
    let src = counter_src(
        "#define BUMP_STMT(obj) do { [obj bump]; } while (0)",
        "\tBUMP_STMT(self);\n\treturn 0;",
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("BUMP_STMT"), "diagnostics: {}", diags);
}

/// A multi-line body, which is what every macro of any size in this
/// repository looks like (`OZ_SLAB_DEFINE`, `oz_assert_msg`, `OZ_AUTO_INIT`).
/// `preproc_arg` keeps the backslashes and newlines, which the probe grammar
/// cannot read, so without stripping continuations first this would parse
/// with errors and be skipped -- a detector blind to precisely the longest
/// bodies.
#[test]
fn message_send_in_multiline_macro_body_rejected() {
    let src = counter_src(
        "#define BUMP_MULTI(obj)                                                        \\\n\
         \tdo {                                                                   \\\n\
         \t\t[obj bump];                                                    \\\n\
         \t} while (0)",
        "\tBUMP_MULTI(self);\n\treturn 0;",
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("BUMP_MULTI"), "diagnostics: {}", diags);
}

/// A boxed string literal is Objective-C; a plain C string is not, and the
/// grammar spells both `string_literal`. `emit::is_boxed_string_literal`
/// is what separates them, so this case and `c_string_in_macro_body_accepted`
/// are a pair -- either alone would pass against a build that got the
/// distinction backwards.
#[test]
fn boxed_string_in_macro_body_rejected() {
    let src = counter_src("#define GREETING @\"hello\"", "\treturn 0;");
    let diags = expect_reject(&src);
    assert!(diags.contains("GREETING"), "diagnostics: {}", diags);
    assert!(diags.contains("boxed string literal"), "diagnostics: {}", diags);
}

#[test]
fn selector_expression_in_macro_body_rejected() {
    let src = counter_src("#define BUMP_SEL @selector(bump)", "\treturn 0;");
    let diags = expect_reject(&src);
    assert!(diags.contains("BUMP_SEL"), "diagnostics: {}", diags);
}

#[test]
fn array_literal_in_macro_body_rejected() {
    let src = counter_src("#define EMPTY_ARRAY @[]", "\treturn 0;");
    let diags = expect_reject(&src);
    assert!(diags.contains("EMPTY_ARRAY"), "diagnostics: {}", diags);
}

// ---------------------------------------------------------------------------
// Accepted: C in a macro body
// ---------------------------------------------------------------------------

/// The C shapes that must NOT be flagged. `arr[i]` matters most: a subscript
/// and a message send are not distinguishable by shape, which is why
/// detection is a probe re-parse with the real grammar rather than a search
/// for brackets.
///
/// This is the case that stands between the rejection and a broken build:
/// a false positive here would fail every sample at once, since
/// `include/oz_sdk` and the samples are full of C macros.
#[test]
fn c_macro_bodies_accepted() {
    let defines = "#define TWICE(x) ((x) * 2)\n\
                   #define PICK(a, i) a[i]\n\
                   #define SUM2(a, i, j) (a[i] + a[j])\n\
                   #define EMAIL \"a@b.com\"\n\
                   #define NILV ((id)0)\n\
                   #define ONCE(x) do { (x)++; } while (0)\n\
                   #define UNUSED_ATTR __attribute__((unused))\n\
                   #define WIDE_MACRO(a, b)                                            \\\n\
                   \tdo {                                                              \\\n\
                   \t\t(a) += (b);                                               \\\n\
                   \t} while (0)";
    let body = "\tint arr[3] = {1, 2, 3};\n\
                \tint i = 0;\n\
                \tint t = TWICE(PICK(arr, i));\n\
                \tint s = SUM2(arr, 0, 1);\n\
                \tONCE(i);\n\
                \tWIDE_MACRO(i, 1);\n\
                \tid nothing = NILV;\n\
                \tif (nothing != NILV) {\n\t\treturn -1;\n\t}\n\
                \tif (EMAIL[1] != '@') {\n\t\treturn -2;\n\t}\n\
                \treturn t + s + i;";
    let src = counter_src(defines, body);
    let out = compile_and_run(&src, "macro_c_bodies");
    // TWICE(arr[0]) = 2, arr[0] + arr[1] = 3, i incremented twice = 2.
    assert_eq!(out, "run=7\n", "output: {}", out);
}

/// A body the grammar cannot read keeps today's behaviour -- emitted
/// verbatim -- rather than becoming a spurious error. tree-sitter is
/// error-tolerant, so without this guard a partially-parsed body could yield
/// a `message_expression` the grammar guessed at from a `[`, and reject a
/// macro containing nothing but C.
#[test]
fn unparseable_macro_body_is_not_flagged() {
    let src = counter_src("#define OPEN_BRACE {\n#define CLOSE_BRACE }", "\treturn 42;");
    let out = compile_and_run(&src, "macro_unparseable_body");
    assert_eq!(out, "run=42\n", "output: {}", out);
}

/// The plain-C half of the `string_literal` pair. See
/// `boxed_string_in_macro_body_rejected`.
#[test]
fn c_string_in_macro_body_accepted() {
    let src = counter_src(
        "#define PLAIN_GREETING \"plain\"",
        "\treturn PLAIN_GREETING[0] == 'p' ? 1 : 0;",
    );
    let out = compile_and_run(&src, "macro_c_string_body");
    assert_eq!(out, "run=1\n", "output: {}", out);
}

// ---------------------------------------------------------------------------
// Already correct: Objective-C as a macro ARGUMENT
// ---------------------------------------------------------------------------

/// Objective-C in a macro *argument* is transpiled where it stands and the
/// macro invocation is preserved unexpanded. That follows from the
/// tree-sitter frontend -- the argument is a real `message_expression` inside
/// a `call_expression` -- and is the deliberate advantage of parsing source
/// rather than a Clang AST, which would have expanded the macro before
/// oz_static ever saw it.
///
/// Pinned on its own account, per #238: nothing else would catch a future
/// change to the walk that started expanding macros, and nothing else would
/// catch this being swept up by the body rejection.
#[test]
fn objc_in_macro_argument_runs() {
    let src = counter_src(
        "#define TWICE(x) ((x) * 2)",
        "\t[self bump];\n\t[self bump];\n\treturn TWICE([self value]);",
    );
    let out = compile_and_run(&src, "macro_argument_runs");
    assert_eq!(out, "run=4\n", "two bumps doubled by the macro: {}", out);
}

/// The same case read at the text level: the invocation must survive as
/// `TWICE(...)` with the send lowered *inside* it. Asserted separately from
/// the behavioural test above because both spellings pass the runtime check
/// -- an implementation that expanded the macro would still print `twice=4`.
#[test]
fn macro_invocation_is_preserved_unexpanded() {
    let src = counter_src("#define TWICE(x) ((x) * 2)", "\treturn TWICE([self value]);");
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("TWICE(Counter_value("),
        "the macro invocation should be preserved with the send lowered inside it:\n{}",
        out.source_c
    );
}
