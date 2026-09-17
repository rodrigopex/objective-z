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
use common::{
    compile_and_run, compile_and_run_strict, iterator_protocol_src, ozarray_src,
    oznumber_src, ozobject_src as PREAMBLE, ozstring_src,
};

fn program(body: &str) -> String {
    format!("/* oz-pool: Thing=4,Holder=2 */\n{}{}\n{}", PREAMBLE(), THING, body)
}

/// The generated C with its echoed-source comments dropped.
///
/// Emission substitutes in place and keeps the author's own line beside
/// the lowered one as a comment, so the *unlowered* spelling is expected
/// to appear in the output -- inside `/* ... */`. An absence assertion
/// that does not strip those can never pass, which cost one round of #531:
/// the loop was already correct and the needle was matching the comment
/// above it.
fn emitted_code(text: &str) -> String {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("/*") || t.starts_with('*') || t.starts_with("//"))
        })
        .collect::<Vec<_>>()
        .join("\n")
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

    let out = oz2c::transpile(&src).expect("should transpile");
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

    let out = oz2c::transpile(&src).expect("should transpile");
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

    let out = oz2c::transpile(&src).expect("should transpile");
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

    let out = oz2c::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("struct OZObject *struct OZObject *"),
        "the id was lowered twice:\n{}",
        out.source_c
    );

    let stdout = compile_and_run(&src, "unlowered_block_param_id");
    assert_eq!(stdout, "called\n");
}

// ---------------------------------------------------------------------
// ARC ownership qualifiers (#428)
//
// Sixth instance of the same asymmetry, and the first found by a
// *compiler CI does not run locally*. `emit` only ever *read*
// `__unsafe_unretained`, as a marker on the source node -- three
// ownership readers ask `node_text(...).contains(...)`. It never stripped
// it on the way out, and oz2c substitutes source text in place, so
// the qualifier travelled verbatim into the generated `.c`. Plain C has no
// such keyword: gcc answers `'__unsafe_unretained' undeclared`.
//
// It was latent for as long as it existed. The only position that ever
// carried one was an ivar block, which `lower_ivar_decl` strips. #428 put
// the qualifier on *locals* -- it is the faithful translation of the
// deleted `released_by_hand`, which held ARC off a variable the author
// released -- and locals are substituted straight through.
//
// **Apple clang accepts `__unsafe_unretained` in plain C mode; Linux gcc
// rejects it.** So `cargo test`, both corpora, ASan, UBSan, both board
// sweeps and `test-pedantic` all passed on macOS over C that is not C.
// That is why the second assertion below exists: on a host whose `cc` is
// clang the compile cannot fail, and something has to.
// ---------------------------------------------------------------------

/// Every position an ARC ownership qualifier can reach, in one program:
/// an ivar, a file-scope declaration, a plain C function's parameter, a
/// plain C struct's field, a method's parameter, a local, a `static`
/// local, a `for`-header declaration, an `id`-typed local, and a local in
/// `main` -- which is the one CI failed on.
///
/// Two assertions, deliberately:
///
///   1. it **compiles and runs**, which is this file's rule and the only
///      check that would have caught the original defect on a gcc host;
///   2. no qualifier survives in the generated text outside a
///      `/* original */` provenance comment, which is the check that
///      fails on a *clang* host too. Neither alone is enough: (1) cannot
///      fail where `cc` is Apple clang, and (2) is a text match, which is
///      exactly the kind of assertion this file's own header warns is
///      weaker than a compile.
///
/// The provenance comments keep the qualifier on purpose -- they quote the
/// author's source, and that is what they are for.
#[test]
fn arc_qualifiers_are_stripped_from_every_emitted_position() {
    let src = format!(
        "/* oz-pool: Qual=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Qual : OZObject {
\tint _tag;
\t__unsafe_unretained Qual *_back;
}
- (instancetype)initWithTag:(int)t;
- (int)tag;
- (int)chain:(__unsafe_unretained Qual *)other;
- (int)sweep;
@end

@implementation Qual
- (instancetype)initWithTag:(int)t {
\tself = [super init];
\tif (self) {
\t\t_tag = t;
\t}
\treturn self;
}
- (int)tag {
\treturn _tag;
}
- (int)chain:(__unsafe_unretained Qual *)other {
\t_back = other;
\treturn [_back tag];
}
- (int)sweep {
\t__unsafe_unretained Qual *loc = self;
\tstatic __unsafe_unretained Qual *slot;
\tslot = loc;
\tint sum = 0;
\tfor (__unsafe_unretained Qual *it = slot; it != 0; it = 0) {
\t\tsum = sum + [it tag];
\t}
\t__unsafe_unretained id anon = loc;
\tsum = sum + [((Qual *)anon) tag];
\treturn sum;
}
@end

struct holder {
\t__unsafe_unretained Qual *field;
};

__unsafe_unretained Qual *g_seen;

static int tag_of(__unsafe_unretained Qual *p)
{
\treturn p != 0 ? [p tag] : -1;
}

#include <stdio.h>
int main(void)
{
\tQual *a = [[Qual alloc] initWithTag:7];
\t__unsafe_unretained Qual *b = a;
\tstruct holder h;
\t/* Stored from `b`, the unretained local, not from `a`: a store of a
\t * reference ARC *manages* into a C struct field is refused outright
\t * (`reject_owning_store_into_c_struct`), and the field's own
\t * qualifier is not consulted. That is a separate wart -- the
\t * diagnostic advises declaring the field `__unsafe_unretained`, which
\t * does not in fact help -- and not this test's subject. */
\th.field = b;
\tg_seen = b;
\tprintf(\"chain=%d\\n\", [a chain:b]);
\tprintf(\"sweep=%d\\n\", [a sweep]);
\tprintf(\"field=%d\\n\", tag_of(h.field));
\tprintf(\"global=%d\\n\", tag_of(g_seen));
\treturn 0;
}
"
    );

    /* (2) first, because it is the one that can fail on this host. */
    let out = oz2c::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected the qualifier program to transpile, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
    for (what, text) in
        [("source", &out.source_c), ("companion header", &out.companion_h), ("companion", &out.companion_c)]
    {
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            /* A provenance comment quotes the author's source and keeps
             * the qualifier on purpose. Every one is either a whole-line
             * `/* ... */` or a `*`-continued banner line, and no emitted
             * declaration begins with either. */
            if trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            assert!(
                !line.contains("__unsafe_unretained"),
                "{} line {} carries an ARC qualifier into the generated C, which is not C \
                 (gcc: `'__unsafe_unretained' undeclared`): {}",
                what,
                n + 1,
                line
            );
        }
    }

    /* (1) and the real assertion: it compiles, links and runs. */
    let stdout =
        compile_and_run_strict(&src, "arc_qualifiers_are_stripped_from_every_emitted_position");
    assert_eq!(stdout, "chain=7\nsweep=14\nfield=7\nglobal=7\n");
}

/// The narrowest reproduction of the CI failure on its own: a single
/// `__unsafe_unretained` local in `main`, which is the shape #428's
/// `behavior_immortal_literals` and `behavior_memory` fixtures use and the
/// one three of them failed on.
///
/// Kept separate from the matrix above so a regression names the position
/// rather than "one of ten".
#[test]
fn an_unsafe_unretained_local_in_main_compiles() {
    let src = format!(
        "/* oz-pool: Counted=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Counted : OZObject
@end
@implementation Counted
@end

#include <stdio.h>
int main(void)
{
\t__unsafe_unretained Counted *c = [Counted alloc];
\tprintf(\"rc=%d\\n\", oz_retain_count(c));
\toz_release((struct OZObject *)c);
\tprintf(\"freed_ok\\n\");
\treturn 0;
}
"
    );
    let stdout = compile_and_run(&src, "an_unsafe_unretained_local_in_main_compiles");
    assert_eq!(stdout, "rc=1\nfreed_ok\n");
}

/* --- #531: the two declaration positions a protocol qualifier survived --- */

/// A protocol-qualified **ivar**, which is the bare-`id` ivar's spelling
/// with a constraint written on it.
///
/// `lower_ivar_decl` is a verbatim text copy with targeted edits, and
/// neither of them matched: `id<Marker>` is a `typedefed_specifier` and not
/// a direct `type_identifier` child, and it is not in a parameter list. So
/// the angle brackets reached the emitted struct and GCC answered `expected
/// identifier or '(' before '<' token`, against a line nobody wrote (#531).
///
/// Normalized to plain `id` -- the preamble typedef the bare form already
/// relies on -- rather than to the root class pointer, so the field's C
/// type is byte-identical to what `id _held;` produces.
#[test]
fn a_protocol_qualified_ivar_is_lowered() {
    let src = format!(
        "/* oz-pool: Thing=4,Holder=2 */\n{}{}{}",
        PREAMBLE(),
        THING,
        "\
#include <stdio.h>

@interface Holder : OZObject {
	id<Marker> _held;
	id _plain;
}
- (void)hold:(Thing *)t;
- (int)sum;
@end
@implementation Holder
- (void)hold:(Thing *)t
{
	_held = t;
	_plain = t;
}
- (int)sum
{
	return [_held tag] + [_plain tag];
}
@end

int main(void)
{
	Thing *t = [[Thing alloc] initWithTag:6];
	Holder *h = [[Holder alloc] init];

	[h hold:t];
	printf(\"sum=%d\\n\", [h sum]);
	return 0;
}
"
    );

    let out = oz2c::transpile(&src).expect("should transpile");
    let code = emitted_code(&out.source_c);
    assert!(
        !code.contains("id<Marker>"),
        "the qualifier reached the emitted struct:\n{}",
        code.lines().filter(|l| l.contains("_held")).collect::<Vec<_>>().join("\n")
    );
    /* Presence beside absence: a field that vanished would also pass the
     * check above (`docs/WORKING.md`, the four green guards). The bare
     * `id _plain;` beside it is what fixes the expected spelling: the
     * qualified field must come out byte-identical to it. */
    for needle in ["id _held;", "id _plain;"] {
        assert!(
            code.contains(needle),
            "expected `{}` on the plain `id` typedef, got:\n{}",
            needle,
            code.lines().filter(|l| l.contains("_held") || l.contains("_plain"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /* And the only assertion that could have caught this: it compiles. */
    let stdout = compile_and_run(&src, "a_protocol_qualified_ivar_is_lowered");
    assert_eq!(stdout, "sum=12\n");
}

/// A protocol-qualified **for-in loop variable**, the second position.
///
/// `forin_binding` has no CST arm at all: it joins the header's type nodes
/// into a string, so `render_type` received the literal text
/// `"id<Marker>"`, fell through to its verbatim return and emitted
/// `for (id<Marker> device = (id<Marker>)OZ_PROTOCOL_SEND_nextObject(...))`
/// -- not C. Fixed in `render_type` itself rather than here, so every
/// text-driven caller is covered at once.
#[test]
fn a_protocol_qualified_forin_variable_is_lowered() {
    let src = format!(
        "/* oz-pool: Thing=4,OZArray=2,OZNumber=2,OZString=2 */\n{}{}{}{}{}{}",
        PREAMBLE(),
        iterator_protocol_src(),
        oznumber_src(),
        ozstring_src(),
        ozarray_src(),
        THING,
    ) + "\
#include <stdio.h>

int main(void)
{
	Thing *a = [[Thing alloc] initWithTag:4];
	Thing *b = [[Thing alloc] initWithTag:5];
	OZArray *devices = @[ a, b ];
	int sum = 0;

	for (id<Marker> device in devices) {
		sum = sum + [device tag];
	}
	printf(\"sum=%d\\n\", sum);
	return 0;
}
";

    let out = oz2c::transpile(&src).expect("should transpile");
    let code = emitted_code(&out.source_c);
    assert!(
        !code.contains("id<Marker>"),
        "the qualifier reached the generated loop:\n{}",
        code.lines().filter(|l| l.contains("device")).collect::<Vec<_>>().join("\n")
    );
    assert!(
        code.contains("for (void * device ="),
        "expected the loop variable lowered to `void *`, got:\n{}",
        out.source_c
            .lines()
            .filter(|l| l.contains("device ="))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let stdout = compile_and_run(&src, "a_protocol_qualified_forin_variable_is_lowered");
    assert_eq!(stdout, "sum=9\n");
}

/// The bare `id` for-in variable, unchanged -- `render_type`'s new arm must
/// widen the `"id"` case rather than replace it.
#[test]
fn a_bare_id_forin_variable_is_unchanged() {
    let src = format!(
        "/* oz-pool: Thing=4,OZArray=2,OZNumber=2,OZString=2 */\n{}{}{}{}{}{}",
        PREAMBLE(),
        iterator_protocol_src(),
        oznumber_src(),
        ozstring_src(),
        ozarray_src(),
        THING,
    ) + "\
#include <stdio.h>

int main(void)
{
	Thing *a = [[Thing alloc] initWithTag:7];
	OZArray *devices = @[ a ];
	int sum = 0;

	for (id device in devices) {
		sum = sum + [device tag];
	}
	printf(\"sum=%d\\n\", sum);
	return 0;
}
";

    let out = oz2c::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("for (void * device ="),
        "expected the bare-`id` loop unchanged, got:\n{}",
        out.source_c
            .lines()
            .filter(|l| l.contains("device ="))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let stdout = compile_and_run(&src, "a_bare_id_forin_variable_is_unchanged");
    assert_eq!(stdout, "sum=7\n");
}

/// A generic whose *argument* is a protocol-qualified `id`
/// (`OZArray<id<Marker> *>`), which must still render as its base class.
///
/// The guard is against `render_type`'s new arm overreaching: a
/// `contains('<')` test would have matched the *argument* and lowered the
/// whole declaration to `void *`. It is a prefix test instead, so a
/// generic, and a class merely named with an `id` prefix, are both left
/// alone.
///
/// Stated honestly about what it reaches: this declaration arrives through
/// `extract_type_and_stars_inner`'s `generic_specifier` arm, which hands
/// `render_type` the base name `OZArray` and not the bracketed text -- so
/// this pins the *outcome*, not the new predicate directly. Nothing in the
/// tree hands the full generic text to `render_type` today; the position
/// that could (`forin_binding`, over a generic-typed loop variable) does
/// not lower a generic at all, before this change or after.
#[test]
fn a_generic_over_a_protocol_qualified_id_is_untouched() {
    let src = format!(
        "/* oz-pool: Badge=4,OZArray=2,OZNumber=2,OZString=2 */\n{}{}{}{}{}{}",
        PREAMBLE(),
        iterator_protocol_src(),
        oznumber_src(),
        ozstring_src(),
        ozarray_src(),
        "\
@interface Badge : OZObject <Marker> {
\tint _tag;
}
- (int)tag;
@end
@implementation Badge
- (int)tag
{
\treturn _tag;
}
@end
",
    ) + "\
#include <stdio.h>

int main(void)
{
\tBadge *a = [Badge alloc];
\tOZArray<id<Marker> *> *devices = @[ a ];

\tprintf(\"count=%u\\n\", (unsigned)[devices count]);
\treturn 0;
}
";

    let out = oz2c::transpile(&src).expect("should transpile");
    let code = emitted_code(&out.source_c);
    assert!(
        code.contains("struct OZArray *devices"),
        "the generic's declared type must still render as its base class, got:\n{}",
        code.lines().filter(|l| l.contains("devices")).collect::<Vec<_>>().join("\n")
    );

    let stdout = compile_and_run(&src, "a_generic_over_a_protocol_qualified_id_is_untouched");
    assert_eq!(stdout, "count=1\n");
}
