// SPDX-License-Identifier: Apache-2.0
//
// free_function_params.rs -- a plain C function's parameters are declared
// types in scope for its body, so a message send to one resolves (#250).
//
// Before this, `emit`'s `function_definition` arm seeded its scope from
// `file_scope_vars` alone and never from the parameter list, so `[w n]` on a
// `Widget *w` parameter was rejected as an `id` receiver -- while the
// identical method `- (int)read:(Widget *)w` resolved fine, because
// `render_method_definition` has always inserted `sig.params`.
//
// Same class of omission as gap Q, where the static bar turned out never to
// scan a plain C function at all: the free-function path kept getting a
// reduced version of what a method body gets. And like #246's fix, this one
// had to be applied in both `emit` and `emit_split`, since the two build the
// same `EmitCtx` separately.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src};

const WIDGET: &str = "\
@interface Widget : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
- (void)bump;
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
- (void)bump
{
	_n = _n + 1;
}
@end
";

/// The case #250 was filed on.
#[test]
fn send_to_an_object_parameter_resolves() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static int readWidget(Widget *w)
{
	return [w n];
}

int main(void) {
	Widget *w = [[Widget alloc] initWithN:21];
	printf(\"n=%d\\n\", readWidget(w));
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "send_to_an_object_parameter_resolves");
    assert_eq!(stdout, "n=21\n");
}

/// A mutating send, so the parameter is not merely read through.
#[test]
fn mutating_send_to_an_object_parameter_resolves() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static void bumpTwice(Widget *w)
{
	[w bump];
	[w bump];
}

int main(void) {
	Widget *w = [[Widget alloc] initWithN:5];
	bumpTwice(w);
	printf(\"n=%d\\n\", [w n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "mutating_send_to_an_object_parameter_resolves");
    assert_eq!(stdout, "n=7\n");
}

/// Several parameters, only one of them an object -- the non-object ones must
/// not disturb the scope. Every parameter is inserted, matching what a method
/// does, so this pins that the mixture behaves.
#[test]
fn mixed_parameters_resolve() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static int combine(int base, Widget *w, int scale)
{
	return base + [w n] * scale;
}

int main(void) {
	Widget *w = [[Widget alloc] initWithN:4];
	printf(\"r=%d\\n\", combine(1, w, 3));
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "mixed_parameters_resolve");
    assert_eq!(stdout, "r=13\n");
}

/// A body declaration shadows a parameter of the same name, which is why the
/// parameters are seeded *before* `collect_local_decls` rather than after.
#[test]
fn a_body_declaration_shadows_a_parameter_of_the_same_name() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static int shadowed(Widget *w)
{
	Widget *inner = [[Widget alloc] initWithN:99];
	return [inner n] - [w n];
}

int main(void) {
	Widget *w = [[Widget alloc] initWithN:9];
	printf(\"d=%d\\n\", shadowed(w));
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "a_body_declaration_shadows_a_parameter");
    assert_eq!(stdout, "d=90\n");
}

/// `main`'s own parameters are ordinary parameters -- and `argv` is not an
/// object, so nothing about it may be treated as one.
#[test]
fn main_with_argc_argv_still_transpiles() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
int main(int argc, char **argv) {
	(void)argv;
	Widget *w = [[Widget alloc] initWithN:argc];
	printf(\"n=%d\\n\", [w n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "main_with_argc_argv_still_transpiles");
    assert_eq!(stdout, "n=1\n");
}

/// The bar is unchanged: an `id` parameter still cannot receive a send, since
/// its class is genuinely unknown. Seeding the scope must not turn "unknown
/// type" into a silent guess -- oz_static never degrades quietly.
#[test]
fn an_id_parameter_is_still_rejected() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
static int readAnything(id thing)
{
	return [thing n];
}

int main(void) {
	Widget *w = [[Widget alloc] initWithN:1];
	return readAnything(w);
}
"
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("cannot statically resolve the receiver type"),
        "an `id` parameter must stay a located error; got:\n{}",
        err
    );
}
