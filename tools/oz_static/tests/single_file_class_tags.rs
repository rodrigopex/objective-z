// SPDX-License-Identifier: Apache-2.0
//
// single_file_class_tags.rs -- a bare class name must get its `struct` tag in
// every position that is copied through from source rather than rebuilt from a
// type: a top-level declaration and a free function's signature (#246), and
// the parameter list of the function a block literal is hoisted into (#326).
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
use common::{compile_and_run, compile_and_run_strict, ozobject_src};

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

/// A third position copied through from source rather than rebuilt from a
/// type: the parameter list of the function a block literal is hoisted into
/// (#326).
///
/// `render_block` synthesizes that function's signature by patching the
/// literal's own parameter list, and until #326 the patch lowered only a
/// type-position `id` (#319) -- a *class* name came through with its bare
/// Objective-C spelling and the hoisted prototype was not valid C at all:
///
/// ```text
/// void oz_block_L200_C24_1(Widget *w);                 /* no `struct` tag */
///         void (*b)(struct Widget *) = oz_block_L200_C24_1;
/// ```
///
/// The declarator side was right all along --
/// `render_block_type_param_list` promotes the name through the ordinary
/// `needs_translation` recursion -- so the two sides also disagreed about the
/// type, which is why running matters here as much as compiling: it says the
/// two are the *same* type rather than two spellings a compiler tolerated.
///
/// Two shapes:
///
///   - `b`: a block variable in a method body, the case as filed
///   - `mixed`: a class name among plain scalars, so the promotion neither
///     misses it nor disturbs its neighbours
///
/// A *file-scope* block variable with a class-typed parameter
/// (`static void (^h)(Widget *) = ^(Widget *wp) { ... };`) is deliberately
/// not here: it is corrupted by a **separate, pre-existing** defect --
/// `class_tag_edits` descends into the literal and hands `apply_edits` a
/// range that overlaps `top_level_block_edits`'s replacement of the whole
/// literal, and overlapping edits truncate each other
/// (`static void (*sHook)(struct Widget *) = oz_block_L213_C34_1 : 0;`).
/// Verified with this fix reverted: byte-identical corruption. Filed
/// separately rather than folded in here.
///
/// No message is sent to the block parameter: a block body shares its
/// enclosing method's flat scope and the parameter is not seeded into it, so
/// a send would be rejected as an `id` receiver. That is a separate gap; what
/// this case is about is the parameter's *type*.
#[test]
fn hoisted_block_parameter_gets_struct_tag() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>

static int gSeen = 0;

@interface Runner : OZObject {
	Widget *_held;
}
- (void)hold:(Widget *)wp;
- (int)run;
@end

@implementation Runner
- (void)hold:(Widget *)wp
{
	_held = wp;
}
- (int)run
{
	void (^b)(Widget *) = ^(Widget *wp) {
		gSeen += wp != 0 ? 1 : 0;
	};
	void (^mixed)(int, Widget *, int) = ^(int seed, Widget *wp, int bump) {
		gSeen += seed + (wp != 0 ? 4 : 0) + bump;
	};
	b(_held);
	mixed(1, _held, 10);
	return gSeen;
}
@end

int main(void) {
	Widget *w = [[Widget alloc] initWithN:3];
	Runner *r = [Runner alloc];
	[r hold:w];
	printf(\"seen=%d\\n\", [r run]);
	return 0;
}
"
    );
    // Compiled and run first, deliberately: without the tag this stops at
    // `must use 'struct' tag to refer to type 'Widget'`, the report's own
    // error -- four of them, a prototype and a definition per literal -- and
    // the most direct statement of the bug. The text assertions below then say
    // *which* spelling it settled on.
    let stdout = compile_and_run(&src, "hoisted_block_parameter_gets_struct_tag");
    assert_eq!(stdout, "seen=16\n");

    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    assert!(
        out.contains("(int seed, struct Widget *wp, int bump)"),
        "the hoisted signature must be tagged:\n{}",
        out
    );
    // The original of a translated line is echoed above it as a `/* ... */`
    // comment, and *that* keeps the Objective-C spelling by design -- so only
    // code lines are checked.
    let leaked: Vec<&str> = out
        .lines()
        .filter(|line| !line.trim_start().starts_with("/*"))
        .filter(|line| line.contains("(Widget *"))
        .collect();
    assert!(
        leaked.is_empty(),
        "a hoisted block parameter kept its bare class name: {:?}\n{}",
        leaked,
        out
    );
}

/// The same shape at **file scope**, which is where two edit passes met and
/// corrupted each other (#331).
///
/// A file-scope block variable is assembled by patching text, and three
/// passes contribute edits to the one `apply_edits` call: `class_tag_edits`
/// tags the class names, `block_pointer_edits` lowers the `^` to a `*`, and
/// `top_level_block_edits` replaces the whole literal with the name of the
/// function `render_block` hoisted it into. The third owns the literal's
/// entire byte range, and `class_tag_edits` was descending into it anyway --
/// so `apply_edits`, which applies back to front, spliced the tail of one
/// replacement into the middle of the other:
///
/// ```text
/// static void (*sHook)(struct Widget *) = oz_block_L213_C34_1 : 0;
/// };
/// ```
///
/// A stray `: 0;` and an orphaned `};` -- not C, and no diagnostic, because
/// nothing could tell a truncated splice from an intended one.
/// `block_pointer_edits` had documented skipping a `block_literal` for
/// exactly this reason since #272; `class_tag_edits` now does the same, and
/// `apply_edits` carries a `debug_assert!` so the next pass to overlap an
/// older one fails at the mistake rather than in the C compiler.
///
/// Run under `compile_and_run_strict`, so the two sides of each
/// initialization have to be the *same* function pointer type rather than
/// two spellings Apple clang merely warns about: the declarator's
/// `void (*)(struct Widget *)` is promoted by
/// `render_block_type_param_list`, while the hoisted prototype's is patched
/// by `class_tag_edits`, and only agreement links.
///
/// Two shapes, matching the in-body case above: a lone class parameter, and
/// a class name among plain scalars.
#[test]
fn file_scope_block_variable_with_class_parameter_compiles_and_runs() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>

static int gHookSeen = 0;

static void (^sHook)(Widget *) = ^(Widget *wp) {
	gHookSeen += wp != 0 ? 1 : 0;
};

static void (^sMixed)(int, Widget *, int) = ^(int seed, Widget *wp, int bump) {
	gHookSeen += seed + (wp != 0 ? 4 : 0) + bump;
};

int main(void) {
	Widget *w = [[Widget alloc] initWithN:3];
	sHook(w);
	sMixed(1, w, 10);
	printf(\"seen=%d n=%d\\n\", gHookSeen, [w n]);
	return 0;
}
"
    );
    // Compiled and run first, deliberately: without the fix this is a hard C
    // syntax error on the corrupted initializer, which is the most direct
    // statement of the bug. The text assertions below then say which spelling
    // it settled on.
    let stdout =
        compile_and_run_strict(&src, "file_scope_block_variable_with_class_parameter");
    assert_eq!(stdout, "seen=16 n=3\n");

    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    // The initializer is the hoisted function's bare name and nothing else:
    // this is the assertion that says the two edits did not truncate each
    // other. Matched loosely on the name, whose `L<line>_C<col>` suffix moves
    // with the preamble.
    let hook: Vec<&str> = out
        .lines()
        .filter(|line| !line.trim_start().starts_with("/*"))
        .filter(|line| line.contains("sHook") && line.contains('='))
        .collect();
    assert_eq!(hook.len(), 1, "expected one `sHook` initialization; got {:?}\n{}", hook, out);
    let hook = hook[0].trim();
    assert!(
        hook.starts_with("static void (*sHook)(struct Widget *) = oz_block_")
            && hook.ends_with(';')
            && !hook.contains(':')
            && !hook.contains('}'),
        "the initializer must be the hoisted name alone; got {:?}\n{}",
        hook,
        out
    );
    // And the hoisted signature on the other side of the `=` agrees, which is
    // what #326 fixed for the in-body form.
    assert!(
        out.contains("(int seed, struct Widget *wp, int bump)"),
        "the hoisted signature must be tagged:\n{}",
        out
    );
    let leaked: Vec<&str> = out
        .lines()
        .filter(|line| !line.trim_start().starts_with("/*"))
        .filter(|line| line.contains("(Widget *") || line.contains('^'))
        .collect();
    assert!(
        leaked.is_empty(),
        "a file-scope block kept its Objective-C spelling: {:?}\n{}",
        leaked,
        out
    );
}
