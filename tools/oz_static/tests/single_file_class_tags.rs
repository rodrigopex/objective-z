// SPDX-License-Identifier: Apache-2.0
//
// single_file_class_tags.rs -- a bare class name must get its `struct` tag in
// the two positions that are copied through from source rather than rebuilt
// from a type: a top-level declaration and a free function's signature (#246).
//
// This is gap A, fixed once in `emit_split` and then found still open in the
// single-file `emit()`, which had no `declaration` arm at all and did not tag
// a function signature either. `emit()` worked by patching the original text,
// so anything no arm claimed survived verbatim -- which is exactly how an
// untagged `static OZHeap *sHeap;` reached the C compiler.
//
// Production was never affected: every real build goes through the CLI, hence
// `emit_split`. What was affected is this suite, which drives
// `oz_static::transpile()` -- so until #246 no Rust test could use a
// file-scope object declaration, the shape `samples/gpio_demo` (`static
// GPIOOutput *led;`), `samples/heap_alloc` (`static OZHeap *sHeap;`) and all
// three singletons are built on. That is why gaps A and D were both diagnosed
// against samples and never locked in by a test.
//
// #254 removed the mechanism: there is one `emit::walk_top_level` now, and
// this arm is reached from both entry points by construction. These cases
// stay as the behavioural pin -- they say the tag is emitted, which is worth
// asserting however the walk is organised.
//
// Each case below therefore asserts the *compiled and run* behaviour, not
// just the emitted text: an untagged declaration is a hard C error, so a
// passing run is the strongest available statement that the tag is there.

mod common;
use common::{compile_and_run, ozobject_src};

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

/// The motivating case: a file-scope `static` holding an object.
///
/// Without the fix the single-file emitter copies `static Widget *g_widget;`
/// through verbatim and the C compiler stops with
/// `must use 'struct' tag to refer to type 'Widget'`.
#[test]
fn file_scope_static_object_declaration_compiles_and_runs() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static Widget *g_widget;

int main(void) {
	g_widget = [[Widget alloc] initWithN:7];
	printf(\"n=%d\\n\", [g_widget n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "file_scope_static_object_declaration");
    assert_eq!(stdout, "n=7\n");
}

/// The same declaration without `static`, since the two spellings reach the
/// grammar differently and gap R was a reminder that a declarator's shape
/// decides whether a check runs at all.
#[test]
fn file_scope_extern_object_declaration_compiles_and_runs() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
Widget *g_plain;

int main(void) {
	g_plain = [[Widget alloc] initWithN:9];
	printf(\"n=%d\\n\", [g_plain n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "file_scope_extern_object_declaration");
    assert_eq!(stdout, "n=9\n");
}

/// The other half of gap A: a free function's signature. `emit()` rendered
/// the body correctly and left the return type untagged, which is the shape
/// `samples/arc_demo`'s `static Sensor *createSensor(int v)` is built on.
#[test]
fn free_function_signature_gets_struct_tag() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static Widget *makeWidget(int v)
{
	return [[Widget alloc] initWithN:v];
}

int main(void) {
	Widget *w = makeWidget(11);
	printf(\"n=%d\\n\", [w n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "free_function_signature_gets_struct_tag");
    assert_eq!(stdout, "n=11\n");
}

/// A parameter, too -- same position class, and it costs nothing to pin.
///
/// The body deliberately does not *send* to the parameter: a free function's
/// parameters are not type-tracked (only file-scope variables seed its
/// scope), so `[w n]` here is rejected as an `id` receiver. That is a
/// separate gap, filed on its own; what this case is about is whether the
/// parameter's type gets its `struct` tag, which the compile answers.
#[test]
fn free_function_parameter_gets_struct_tag() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static int haveWidget(Widget *w)
{
	return w != nil;
}

int main(void) {
	Widget *w = [[Widget alloc] initWithN:13];
	printf(\"have=%d n=%d\\n\", haveWidget(w), [w n]);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    assert!(
        out.contains("haveWidget(struct Widget *w)"),
        "parameter type must be tagged; got:\n{}",
        out
    );
    let stdout = compile_and_run(&src, "free_function_parameter_gets_struct_tag");
    assert_eq!(stdout, "have=1 n=13\n");
}

/// An already-tagged declaration must not be tagged twice. `class_tag_edits`
/// returns early inside a `struct_specifier`, so this is a guard against that
/// early return being lost rather than a live bug.
///
/// No message is sent through `g_tagged`: `file_scope_vars` recognises only
/// the *untagged* spelling, so writing `struct Widget *` by hand costs the
/// variable its type tracking and a send to it is rejected as `id`. Also a
/// separate gap, also filed -- and a good reason not to write the tag by
/// hand, since the transpiler adds it for you.
#[test]
fn already_tagged_declaration_is_left_alone() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static struct Widget *g_tagged;

int main(void) {
	g_tagged = [[Widget alloc] initWithN:5];
	printf(\"set=%d\\n\", g_tagged != nil);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    assert!(
        !out.contains("struct struct Widget"),
        "double-tagged a declaration that already had its tag:\n{}",
        out
    );
    let stdout = compile_and_run(&src, "already_tagged_declaration_is_left_alone");
    assert_eq!(stdout, "set=1\n");
}

/// A non-class type of the same shape must be untouched -- `is_class` is the
/// gate, and `struct point` and `struct Widget` are spelled alike. This is
/// the mistake gap F recorded twice (plain C member access read as dot
/// syntax, and the same hole latent in subscripting).
#[test]
fn non_class_type_names_are_not_tagged() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
typedef struct { int x; } point;
static point g_point;

int main(void) {
	g_point.x = 3;
	printf(\"x=%d\\n\", g_point.x);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    assert!(
        !out.contains("struct point g_point"),
        "tagged a plain C typedef as if it were a class:\n{}",
        out
    );
    let stdout = compile_and_run(&src, "non_class_type_names_are_not_tagged");
    assert_eq!(stdout, "x=3\n");
}
