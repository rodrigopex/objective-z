// SPDX-License-Identifier: Apache-2.0
//
// free_function_return_types.rs -- the temporary a free function's `return`
// is evaluated into carries that function's declared return type (#336).
//
// `render_return_statement` puts the returned value in a temporary whenever
// something has to run *after* it is evaluated but *before* the function
// leaves -- a pending ARC release, or an `@synchronized` unlock. The
// temporary's type comes from `EmitCtx::method_return_type`, which
// `render_method_definition` records for a method and which the
// `function_definition` arm did not record at all: a free function kept
// `EmitCtx::new`'s `"int"` placeholder whatever it actually returned.
//
// Another instance of the pattern `free_function_params.rs` and gap Q
// describe -- the free-function path getting a reduced version of what a
// method body gets -- and the same reason no gate saw it: no sample or
// corpus case has a free function that both returns something other than an
// `int` and needs a cleanup at its `return`.
//
// Two things worth stating about how these cases are written.
//
// The shapes here take **no cast**. `return (T *)s;` used to reach the
// cleanup path and would have been the obvious way to write this, but #332
// routes a cast return back to the fast path, so a cast-based case would
// pass with the bug still in place. What forces the cleanup path is a second
// owned local that is *not* the returned one, so ARC owes it a release.
//
// The assertions are on **compiled and run** behaviour, not only on the
// emitted text, because the failure is not uniform across targets: `int` and
// a pointer are both 32 bits on ARM, so there the wrong output is a bare
// constraint violation that a lenient compiler lets through with the value
// intact, while on a 64-bit host it truncates. A `double` narrowed to `int`
// is not even a constraint violation -- it is a legal implicit conversion
// that silently rounds, and nothing but running the code catches it.

mod common;
use common::{compile_and_run_strict, ozobject_src};

const WIDGET: &str = "\
@interface Widget : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
@end
@implementation Widget
- (id)initWithN:(int)n
{
	self = [super init];
	if (self != nil) {
		_n = n;
	}
	return self;
}
- (int)n
{
	return _n;
}
@end
";

fn program(body: &str) -> String {
    format!("{}{}\n{}", ozobject_src(), WIDGET, body)
}

/// The declared type of the synthesized return temporary whose initializer
/// is `value`, i.e. everything on that line before the temporary's name.
///
/// Located by its initializer rather than by its name, which carries a
/// line number of the *merged* buffer and so moves whenever anything above
/// it in `ozobject_src()` does.
fn return_temporary_type(source_c: &str, value: &str) -> String {
    let needle = format!("= {};", value);
    let line = source_c
        .lines()
        .find(|l| l.contains("_oz_sync_ret_") && l.trim_end().ends_with(&needle))
        .unwrap_or_else(|| {
            panic!("no synthesized return temporary initialized from `{}` in:\n{}", value, source_c)
        });
    let name_start = line.find("_oz_sync_ret_").unwrap();
    line[..name_start].trim().to_string()
}

/// No synthesized return temporary was initialized from `value`.
///
/// Per-initializer rather than a blanket "the file contains no
/// `_oz_sync_ret_`", because a `main` that owns an object legitimately gets
/// one for its own `return 0;`.
fn assert_no_return_temporary(source_c: &str, value: &str) {
    let needle = format!("= {};", value);
    let found = source_c
        .lines()
        .find(|l| l.contains("_oz_sync_ret_") && l.trim_end().ends_with(&needle));
    assert!(
        found.is_none(),
        "`return {};` needs no cleanup and must stay on the fast path, but got `{}` in:\n{}",
        value,
        found.unwrap().trim(),
        source_c
    );
}

/// The case #336 was filed on: a free function returning a class pointer,
/// with a second owned local ARC has to release on the way out.
///
/// Without the fix the emitted C is `int _oz_sync_ret_... = w;` followed by
/// `return _oz_sync_ret_...;` out of a `struct Widget *` function -- two
/// `-Wint-conversion` constraint violations, and a truncated pointer on a
/// 64-bit host, so the run reads through a mangled address.
#[test]
fn pointer_return_with_a_pending_release_keeps_its_pointer_type() {
    let src = program(
        "\
#include <stdio.h>

Widget *makeWidget(int n)
{
	Widget *scratch = [[Widget alloc] initWithN:1];
	Widget *result = [[Widget alloc] initWithN:n];
	int seen = [scratch n];
	printf(\"scratch=%d\\n\", seen);
	return result;
}

int main(void)
{
	Widget *w = makeWidget(41);
	printf(\"n=%d\\n\", [w n]);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert_eq!(
        return_temporary_type(&out.source_c, "result"),
        "struct Widget *",
        "the return temporary must carry the function's own type, not `int`; got:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "free_fn_return_pointer");
    assert_eq!(stdout, "scratch=1\nn=41\n");
}

/// The same omission on plain C return types, where `"int"` is equally
/// wrong and mostly *not* a constraint violation.
///
/// `size_t` is the truncating case (a value above 2^32 loses its high
/// half), `double` the silently rounding one (1.5 becomes 1), and
/// `const char *` a second pointer shape whose spelling has to survive the
/// round trip -- a `char *` temporary would drop the `const` and warn on
/// the way back out.
#[test]
fn plain_c_return_types_are_not_narrowed_to_int() {
    let src = program(
        "\
#include <stdio.h>
#include <stddef.h>

size_t bigCount(void)
{
	Widget *scratch = [[Widget alloc] initWithN:1];
	printf(\"scratch=%d\\n\", [scratch n]);
	return (size_t)5000000000ULL;
}

double half(void)
{
	Widget *scratch = [[Widget alloc] initWithN:2];
	printf(\"scratch=%d\\n\", [scratch n]);
	return 1.5;
}

const char *label(void)
{
	Widget *scratch = [[Widget alloc] initWithN:3];
	printf(\"scratch=%d\\n\", [scratch n]);
	return \"objz\";
}

int main(void)
{
	printf(\"count=%llu\\n\", (unsigned long long)bigCount());
	printf(\"half=%.1f\\n\", half());
	printf(\"label=%s\\n\", label());
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    for (value, expected) in
        [("(size_t)(5000000000ULL)", "size_t"), ("1.5", "double"), ("\"objz\"", "const char*")]
    {
        assert_eq!(
            return_temporary_type(&out.source_c, value),
            expected,
            "the temporary returning `{}` must be a `{}`, not an `int`; got:\n{}",
            value,
            expected,
            out.source_c
        );
    }

    let stdout = compile_and_run_strict(&src, "free_fn_return_plain_c");
    assert_eq!(
        stdout,
        "scratch=1\ncount=5000000000\nscratch=2\nhalf=1.5\nscratch=3\nlabel=objz\n"
    );
}

/// A `struct` returned by value, which cannot be converted to an `int` at
/// all -- with the bug this is not a warning anywhere, it is a hard
/// "initializing 'int' with an expression of incompatible type 'struct
/// point'". The `struct` keyword has to be in the temporary's type too: a
/// bare tag name does not name a type in C.
#[test]
fn struct_by_value_return_with_a_pending_release_keeps_its_type() {
    let src = program(
        "\
#include <stdio.h>

struct point {
	int x;
	int y;
};

struct point origin(int bias)
{
	Widget *scratch = [[Widget alloc] initWithN:4];
	struct point p;
	p.x = bias + [scratch n];
	p.y = bias - [scratch n];
	return p;
}

int main(void)
{
	struct point p = origin(10);
	printf(\"x=%d y=%d\\n\", p.x, p.y);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert_eq!(
        return_temporary_type(&out.source_c, "p"),
        "struct point",
        "the return temporary must be typed `struct point`, keyword included; got:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "free_fn_return_struct");
    assert_eq!(stdout, "x=14 y=6\n");
}

/// A `void` function's `return;` has no value, so there is nothing to
/// evaluate into a temporary and none must be synthesized -- the cleanups
/// simply run and then it leaves. This held before the fix and has to keep
/// holding: reading the return type is not licence to declare a `void`
/// variable.
#[test]
fn void_return_with_a_pending_release_synthesizes_no_temporary() {
    let src = program(
        "\
#include <stdio.h>

void report(int n)
{
	Widget *scratch = [[Widget alloc] initWithN:n];
	printf(\"n=%d\\n\", [scratch n]);
	return;
}

int main(void)
{
	report(9);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("_oz_sync_ret_"),
        "a valueless `return` must synthesize no temporary; got:\n{}",
        out.source_c
    );
    assert!(
        out.source_c.contains("oz_static_release((struct OZObject *)(scratch));"),
        "the pending release must still run before the return; got:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "free_fn_return_void");
    assert_eq!(stdout, "n=9\n");
}

/// A `return` with nothing to clean up after it stays on the fast path,
/// where the statement is copied through byte for byte. Recording the
/// return type must not drag such a function onto the temporary path.
#[test]
fn a_return_needing_no_cleanup_stays_on_the_fast_path() {
    let src = program(
        "\
#include <stdio.h>

const char *tag(void)
{
	return \"fast\";
}

int addTwo(int a, int b)
{
	return a + b;
}

int widgetN(Widget *w)
{
	return [w n];
}

int main(void)
{
	Widget *w = [[Widget alloc] initWithN:5];
	printf(\"tag=%s sum=%d n=%d\\n\", tag(), addTwo(2, 3), widgetN(w));
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    for value in ["\"fast\"", "a + b", "Widget_n((struct Widget *)(w))"] {
        assert_no_return_temporary(&out.source_c, value);
    }
    assert!(
        out.source_c.contains("\treturn \"fast\";\n") && out.source_c.contains("\treturn a + b;\n"),
        "an untranslated return must be copied through verbatim; got:\n{}",
        out.source_c
    );
    // A translated one (`return [w n];`) still takes the fast path -- it is
    // rebuilt in place, not evaluated into a temporary.
    assert!(
        out.source_c.contains("\treturn Widget_n((struct Widget *)(w));\n"),
        "a translated return with no cleanups must be rebuilt in place; got:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "free_fn_return_fast_path");
    assert_eq!(stdout, "tag=fast sum=5 n=5\n");
}
