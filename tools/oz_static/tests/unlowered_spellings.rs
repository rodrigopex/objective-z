// SPDX-License-Identifier: Apache-2.0
//
// unlowered_spellings.rs -- a type that reaches the output must be lowered
// wherever it appears, not only where someone remembered (#367).
//
// Two positions carried a type through to the generated C untouched, so
// the transpile reported success and the *compiler* rejected the output:
//
//   - a class-typed field in a file-scope C struct. The definition is
//     hoisted into the companion header, and it was pushed as
//     `node_text(node, source)` -- the author's bytes -- so `Thing *held`
//     arrived with no `struct` tag: `unknown type name 'Thing'`. No store
//     and no read needed; the declaration alone broke the build.
//   - `id` or `id<Proto>` as a plain C function's parameter. A method's
//     equivalent lowers to `void *` and a block literal's to the root
//     class pointer; a free function's was copied through and GCC answered
//     `expected ')'`.
//
// Fourth and fifth instance of one asymmetry: #326 lowered class-typed
// *block parameters*, #336 recorded a free function's *return type*, and
// each time the thing methods already had was missing for free functions
// and file scope. Which is why both fixes here are applied where the type
// is *carried*, and the `id` one by teaching the shared predicate both
// spellings rather than the one call site that reported it -- a block
// literal's parameter list asks the same predicate and would otherwise
// have kept the same hole.
//
// Every case here **compiles** the generated C. That is the point: all
// five of these defects were visible in output that had never been through
// a compiler, and this one was found only when a test finally built the
// shape. `compile_and_run` is the assertion, not a text match.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

fn program(body: &str) -> String {
    format!("/* oz-pool: Thing=4,Holder=2 */\n{}{}\n{}", PREAMBLE(), THING, body)
}

const THING: &str = "\
@interface Thing : OZObject {
	int _tag;
}
- (id)initWithTag:(int)tag;
- (int)tag;
@end
@implementation Thing
- (id)initWithTag:(int)tag
{
	self = [super init];
	if (self != nil) {
		_tag = tag;
	}
	return self;
}
- (int)tag
{
	return _tag;
}
@end

@protocol Marker
- (int)tag;
@end
";

/// A class-typed field in a file-scope C struct is tagged, so the struct
/// the companion header hoists is valid C.
///
/// The store is of a *borrowed* reference, which is the only kind #359
/// permits into a C struct field -- storing a managed one is a located
/// error there, because nothing can release it when the struct dies. The
/// two fixes compose exactly that way: #367 makes the declaration
/// expressible, #359 decides what may be put in it.
#[test]
fn a_class_typed_struct_field_is_tagged() {
    let src = program(
        "\
#include <stdio.h>

struct box {
	Thing *held;
	int n;
};

static struct box g_box;

static void fill(Thing *borrowed)
{
	g_box.held = borrowed;
	g_box.n = 5;
}

int main(void)
{
	Thing *owner = [[Thing alloc] initWithTag:7];

	fill(owner);
	printf(\"held=%d n=%d\\n\", [g_box.held tag], g_box.n);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.companion_h.contains("struct Thing *held"),
        "the field must carry the struct tag into the companion header:\n{}",
        out.companion_h
            .lines()
            .filter(|l| l.contains("held"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    /* And it compiles, which is the whole complaint. */
    let stdout = compile_and_run(&src, "unlowered_struct_field");
    assert_eq!(stdout, "held=7 n=5\n");
}

/// The tagged spelling was already correct and must stay byte-identical --
/// the fix must not produce `struct struct Thing *`.
#[test]
fn an_already_tagged_struct_field_is_untouched() {
    let src = program(
        "\
struct tagged {
	struct Thing *held;
};

static struct tagged g_t;

void fill(Thing *t)
{
	g_t.held = t;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.companion_h.contains("struct Thing *held"),
        "expected the tag once:\n{}",
        out.companion_h
    );
    assert!(
        !out.companion_h.contains("struct struct"),
        "the tag was applied twice:\n{}",
        out.companion_h
    );
}

/// `id` and `id<Proto>` as a plain C function's parameters lower the way a
/// method's do.
#[test]
fn an_id_parameter_on_a_plain_c_function_is_lowered() {
    let src = program(
        "\
#include <stdio.h>

static int markerTag(id<Marker> m)
{
	return [m tag];
}

static int bareIdTag(id thing)
{
	return [(Thing *)thing tag];
}

int main(void)
{
	Thing *t = [[Thing alloc] initWithTag:3];

	printf(\"qualified=%d bare=%d\\n\", markerTag(t), bareIdTag(t));
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    for needle in ["markerTag(struct OZObject *", "bareIdTag(struct OZObject *"] {
        assert!(
            out.source_c.contains(needle),
            "expected `{}` in the signature, got:\n{}",
            needle,
            out.source_c
                .lines()
                .filter(|l| l.contains("Tag("))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let stdout = compile_and_run(&src, "unlowered_id_param");
    assert_eq!(stdout, "qualified=3 bare=3\n");
}

/// A block-typed parameter carrying an `id` still works, and is the case
/// that first showed the two lowerings can reach the same bytes.
///
/// `block_pointer_edits` lowers the `id` inside `void (^cb)(id)` and the
/// signature-wide pass now does too, so the edit list contains the same
/// replacement twice at the same range. `apply_edits` refuses overlaps --
/// rightly -- so the duplicates are dropped at the call site rather than by
/// relaxing that assertion, and this test is what keeps that path exercised.
#[test]
fn a_block_parameter_carrying_an_id_still_lowers_once() {
    let src = program(
        "\
#include <stdio.h>

static void takeCb(void (^cb)(id))
{
	cb(0);
}

int main(void)
{
	takeCb(^(id anything) {
		(void)anything;
		printf(\"called\\n\");
	});
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("struct OZObject *struct OZObject *"),
        "the id was lowered twice:\n{}",
        out.source_c
    );

    let stdout = compile_and_run(&src, "unlowered_block_param_id");
    assert_eq!(stdout, "called\n");
}
