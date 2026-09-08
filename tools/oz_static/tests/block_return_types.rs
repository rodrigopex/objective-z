// SPDX-License-Identifier: Apache-2.0
//
// block_return_types.rs -- the temporary a `return` inside a block literal
// is evaluated into carries the *block's* declared return type, not the
// enclosing body's (#339).
//
// The same omission `free_function_return_types.rs` covers for free
// functions (#336), one layer down. `render_return_statement` types its
// synthesized temporary from `EmitCtx::method_return_type`, which whoever
// renders a body has to record for that body. `render_block` computes the
// block's return type for the hoisted function's signature and, until this
// fix, left the field holding the *enclosing* body's -- so a block
// returning a `Widget *` inside an `-(int)` method declared its temporary
// `int`.
//
// A block literal differs from the other two recording paths in one way
// that matters: it is rendered *inside* an enclosing body's context, on the
// same `EmitCtx`, rather than owning a fresh one. So it has to save, set,
// render and put the enclosing type back -- otherwise the next `return` in
// the enclosing body, after the literal, takes the block's type instead.
// `a_return_after_a_block_literal_keeps_the_enclosing_bodys_type` is that
// case, and a fix that sets without restoring fails it.
//
// The same two notes as the free-function file apply to how these cases are
// written.
//
// The shapes take **no cast**: #332 routes a cast return back to the fast
// path, so `return (Widget *)w;` would pass with the bug still in place.
// What forces the cleanup path is a second owned local that is *not* the
// returned one, so ARC owes it a release at the `return`.
//
// The assertions are on **compiled and run** behaviour as well as on the
// emitted text, because the failure is not uniform across targets: `int`
// and a pointer are both 32 bits on ARM, so there the wrong output is a
// bare constraint violation a lenient compiler lets through with the value
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

/// The case #339 was filed on: a block returning a class pointer inside a
/// method returning `int`, with a second owned local ARC has to release on
/// the way out of the block.
///
/// Without the fix the hoisted block function contains
/// `int _oz_sync_ret_... = result;` and returns it out of a
/// `struct Widget *` function -- two `-Wint-conversion` constraint
/// violations, and a truncated pointer on a 64-bit host, so the caller
/// reads through a mangled address.
#[test]
fn a_pointer_returning_block_inside_an_int_method_keeps_its_pointer_type() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	Widget *(^mk)(void) = ^Widget *(void) {
		Widget *scratch = [[Widget alloc] initWithN:1];
		Widget *result = [[Widget alloc] initWithN:41];
		printf(\"scratch=%d\\n\", [scratch n]);
		return result;
	};
	Widget *w = mk();
	printf(\"n=%d\\n\", [w n]);
	return 0;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run];
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert_eq!(
        return_temporary_type(&out.source_c, "result"),
        "struct Widget *",
        "the block's return temporary must carry the block's own type, not the enclosing \
         method's `int`; got:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "block_return_pointer");
    assert_eq!(stdout, "scratch=1\nn=41\n");
}

/// A block returning a **non-pointer, non-`int`** type, where the wrong
/// type is a silent legal conversion rather than a constraint violation.
///
/// `double` narrowed to `int` rounds and no compiler is obliged to say a
/// word, so the emitted-text assertion is the only static evidence and the
/// run is the only behavioural one. `size_t` is the truncating shape (a
/// value above 2^32 loses its high half through an `int`) and
/// `const char *` a pointer whose spelling has to survive the round trip --
/// a `char *` temporary would drop the `const` and warn on the way back
/// out.
#[test]
fn a_blocks_plain_c_return_type_is_not_narrowed_to_the_enclosing_int() {
    let src = program(
        "\
#include <stdio.h>
#include <stddef.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	double (^half)(void) = ^double(void) {
		Widget *scratch = [[Widget alloc] initWithN:2];
		printf(\"scratch=%d\\n\", [scratch n]);
		return 1.5;
	};
	size_t (^big)(void) = ^size_t(void) {
		Widget *scratch = [[Widget alloc] initWithN:3];
		printf(\"scratch=%d\\n\", [scratch n]);
		return (size_t)5000000000ULL;
	};
	const char *(^label)(void) = ^const char *(void) {
		Widget *scratch = [[Widget alloc] initWithN:4];
		printf(\"scratch=%d\\n\", [scratch n]);
		return \"objz\";
	};

	printf(\"half=%.1f\\n\", half());
	printf(\"count=%llu\\n\", (unsigned long long)big());
	printf(\"label=%s\\n\", label());
	return 0;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run];
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    for (value, expected) in
        [("1.5", "double"), ("(size_t)(5000000000ULL)", "size_t"), ("\"objz\"", "const char*")]
    {
        assert_eq!(
            return_temporary_type(&out.source_c, value),
            expected,
            "the block temporary returning `{}` must be a `{}`, not the enclosing method's \
             `int`; got:\n{}",
            value,
            expected,
            out.source_c
        );
    }

    let stdout = compile_and_run_strict(&src, "block_return_plain_c");
    assert_eq!(
        stdout,
        "scratch=2\nhalf=1.5\nscratch=3\ncount=5000000000\nscratch=4\nlabel=objz\n"
    );
}

/// The restore. A block literal is rendered on the enclosing body's
/// `EmitCtx`, so recording the block's return type without putting the
/// enclosing body's back leaves every later `return` in that body typed
/// from the block.
///
/// The enclosing method returns `Widget *` and its own `return` is on the
/// cleanup path (`scratch` is owed a release), and it is written *after* a
/// block literal returning `double`. A set-without-restore emits
/// `double _oz_sync_ret_... = result;` for the method -- initializing a
/// `double` from a `struct Widget *`, which is not a conversion C has at
/// all -- and the returned pointer never survives.
///
/// The enclosing body's owned locals are declared *after* the literal on
/// purpose. Block bodies share their enclosing body's flat ARC scope (a
/// known spike simplification, noted in `render_block`), so an owned local
/// declared ahead of the literal is one the block's own `return` emits a
/// release for -- naming, in the hoisted function, a variable that only
/// exists in the enclosing one. Declaring them after keeps this case about
/// the return *type* rather than about that.
#[test]
fn a_return_after_a_block_literal_keeps_the_enclosing_bodys_type() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (Widget *)pick;
@end
@implementation Host
- (Widget *)pick
{
	double (^ratio)(void) = ^double(void) {
		Widget *innerScratch = [[Widget alloc] initWithN:6];
		Widget *inner = [[Widget alloc] initWithN:7];
		printf(\"inner=%d %d\\n\", [innerScratch n], [inner n]);
		return 2.5;
	};

	Widget *scratch = [[Widget alloc] initWithN:5];
	Widget *result = [[Widget alloc] initWithN:37];

	printf(\"ratio=%.1f\\n\", ratio());
	printf(\"scratch=%d\\n\", [scratch n]);
	return result;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	Widget *w = [h pick];
	printf(\"n=%d\\n\", [w n]);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert_eq!(
        return_temporary_type(&out.source_c, "2.5"),
        "double",
        "the block's temporary must be typed from the block; got:\n{}",
        out.source_c
    );
    assert_eq!(
        return_temporary_type(&out.source_c, "result"),
        "struct Widget *",
        "the enclosing method's temporary, written after the block literal, must be typed from \
         the method -- the block's type has to be put back; got:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "block_return_restore");
    assert_eq!(stdout, "inner=6 7\nratio=2.5\nscratch=5\nn=37\n");
}

/// A `void` block's `return;` has no value, so nothing is evaluated into a
/// temporary and none must be synthesized -- the cleanups run and it
/// leaves. Recording a `void` return type is not licence to declare a
/// `void` variable.
#[test]
fn a_void_block_with_a_pending_release_synthesizes_no_temporary() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	void (^report)(int) = ^(int n) {
		Widget *scratch = [[Widget alloc] initWithN:n];
		printf(\"n=%d\\n\", [scratch n]);
		return;
	};
	report(9);
	return 0;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run];
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    // Per-statement rather than a blanket "the file has no
    // `_oz_sync_ret_`": `main` owns the receiver and legitimately gets one
    // for its own `return [h run];`.
    assert!(
        out.source_c
            .contains("oz_static_release((struct OZObject *)(scratch));\n\treturn;\n"),
        "a valueless `return` must synthesize no temporary and simply follow the pending \
         release; got:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "block_return_void");
    assert_eq!(stdout, "n=9\n");
}
