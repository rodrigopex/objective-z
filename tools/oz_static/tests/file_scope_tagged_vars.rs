// SPDX-License-Identifier: Apache-2.0
//
// file_scope_tagged_vars.rs -- a file-scope object variable is type-tracked
// whichever way its type is spelled (#251).
//
// `static Widget *g;` gives a `type_identifier`, so
// `collect::extract_type_and_stars` reports `Widget`. `static struct Widget
// *g;` goes through that function's `struct_specifier` arm and reports
// `struct Widget`, which is not a key in the known-classes set -- so
// `file_scope_vars` skipped the declaration and a later send reported its
// receiver as `id`.
//
// The identical *local* always resolved, because `collect_local_decls` has no
// such membership gate. Two places disagreeing about what counts as an object
// declaration is the same asymmetry behind gap R (`staticbar` vs
// `collect_local_decls` on what counts as a local) and #246 (`emit` vs
// `emit_split` on tagging), which is the reason this one was worth fixing
// rather than filing as a curiosity: the bug is the disagreement, not the
// spelling.

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

/// The case #251 was filed on.
#[test]
fn tagged_file_scope_static_resolves_a_send() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static struct Widget *g_tagged;

int main(void) {
	g_tagged = [[Widget alloc] initWithN:31];
	printf(\"n=%d\\n\", [g_tagged n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "tagged_file_scope_static_resolves_a_send");
    assert_eq!(stdout, "n=31\n");
}

/// The untagged spelling, which always worked -- kept beside the tagged one so
/// the pair is the assertion. Asserting only the tagged form would pass just as
/// well against a build that had broken the untagged one.
#[test]
fn untagged_file_scope_static_resolves_a_send() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
static Widget *g_bare;

int main(void) {
	g_bare = [[Widget alloc] initWithN:32];
	printf(\"n=%d\\n\", [g_bare n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "untagged_file_scope_static_resolves_a_send");
    assert_eq!(stdout, "n=32\n");
}

/// Not `static`, and tagged: the storage class is a separate axis from the
/// type spelling, and the gate this fixes sat on the type.
#[test]
fn tagged_file_scope_extern_resolves_a_send() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
struct Widget *g_plain_tagged;

int main(void) {
	g_plain_tagged = [[Widget alloc] initWithN:33];
	printf(\"n=%d\\n\", [g_plain_tagged n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "tagged_file_scope_extern_resolves_a_send");
    assert_eq!(stdout, "n=33\n");
}

/// The tag is not added twice. `render_type` re-adds it, so the bare name has
/// to be what reaches it -- passing `struct Widget` through would give
/// `struct struct Widget *`.
#[test]
fn tagged_declaration_is_not_double_tagged() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
static struct Widget *g_tagged;
int main(void) {
	g_tagged = [[Widget alloc] initWithN:1];
	return [g_tagged n];
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    assert!(
        !out.contains("struct struct"),
        "double-tagged the declaration:\n{}",
        out
    );
}

/// A plain C struct at file scope must still be ignored: stripping the
/// `struct ` prefix must not make `is_class`/`known` any less the authority on
/// whether the name is a class. This is the mistake gap F recorded twice --
/// `struct point` and `struct Widget` are spelled alike.
#[test]
fn a_plain_c_struct_at_file_scope_is_not_treated_as_an_object() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        WIDGET,
        "\
#include <stdio.h>
struct point { int x; };
static struct point *g_point;
static struct point storage;

int main(void) {
	g_point = &storage;
	g_point->x = 4;
	printf(\"x=%d\\n\", g_point->x);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "plain_c_struct_at_file_scope");
    assert_eq!(stdout, "x=4\n");
}
