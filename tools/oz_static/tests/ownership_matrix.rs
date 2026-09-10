// SPDX-License-Identifier: Apache-2.0
//
// ownership_matrix.rs -- every sink a `+1` reference can reach, and what
// the emitter does with it.
//
// This is an audit turned into a gate. The audit behind #359 walked 25
// shapes and found six defects, three of them use-after-free reachable
// from ordinary Objective-C; the ones this file pins as *correct* were
// correct only because someone checked, and nothing stopped them
// regressing between checks. Ownership is the property people trust this
// backend for -- an ARC that can free a live object is worse than no ARC,
// because code gets written on top of it -- so the whole matrix is
// asserted rather than the handful of shapes each fix happened to touch.
//
// What is asserted is the *shape* of the refcount traffic in one emitted
// function: how many allocations, retains and releases it contains. That
// is deliberately coarse. It cannot tell a correct release from a
// correctly-shaped wrong one, so it is a regression net and not a proof --
// the behavioural proof is the corpus running under ASan and LSan
// (`just test-behavior`), and the per-shape reasoning lives in the
// dedicated files (`arc_leak_regressions.rs`, `return_alias_escape.rs`,
// `explicit_ivar_store.rs`, `strong_slots.rs`).
//
// **Known defects are asserted to still be defective.** `KNOWN_DEFECTS`
// works the way `KNOWN_CC_FAILURES` does in `corpus_parity.rs`: fixing one
// fails this test, which forces the entry to be removed in the same change
// -- so the list cannot rot into a set of silently-skipped shapes, and a
// reader can trust that everything *not* listed is believed correct.

mod common;
use common::ozobject_src as PREAMBLE;

/// One shape: the source that produces it, the generated function to look
/// at, and the refcount traffic expected inside that function.
struct Shape {
    /// What the shape is, in the words the audit used.
    what: &'static str,
    /// The generated C function to count inside.
    func: &'static str,
    /// (allocations, retains, releases) expected in that function.
    expect: (usize, usize, usize),
    /// `Some(issue)` when the expectation above is the *defect* rather
    /// than the correct answer -- asserted so that fixing it fails here.
    known_defect: Option<&'static str>,
}

/// Shared decls every shape's body may use.
const DECLS: &str = "\
@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag
{
	return 1;
}
@end

@protocol Supplier
- (Thing *)supply;
@end

@interface Maker : OZObject
@end
@implementation Maker
- (Thing *)supply
{
	return [[Thing alloc] init];
}
@end

@interface Holder : OZObject {
	Thing *_ivar;
	Thing *_arr[2];
}
@end
@implementation Holder
@end

static Thing *g_global;
Thing *factory(void);
void keep(Thing *t);
";

fn program(body: &str) -> String {
    format!("/* oz-pool: Thing=8,Holder=2,Maker=2 */\n{}{}\n{}", PREAMBLE(), DECLS, body)
}

/// One generated C function, by brace matching.
///
/// Not "up to the first `\n}`": that cuts short every function containing
/// a nested block whose closing brace lands in column zero -- which is
/// every `@synchronized` and every scoped group. During the audit that
/// mistake made a correctly balanced function look like it leaked, and the
/// false finding survived until the extractor was fixed.
fn function_body(source_c: &str, name: &str) -> String {
    let needle = format!("{}(", name);
    let mut from = 0;
    while let Some(rel) = source_c[from..].find(&needle) {
        let at = from + rel;
        let line_end = source_c[at..].find('\n').map(|e| at + e).unwrap_or(source_c.len());
        if !source_c[at..line_end].trim_end().ends_with(';') {
            let open = source_c[at..].find('{').map(|o| at + o).expect("a definition has a body");
            let mut depth = 0usize;
            for (i, ch) in source_c[open..].char_indices() {
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                    if depth == 0 {
                        return source_c[at..open + i + 1].to_string();
                    }
                }
            }
        }
        from = at + needle.len();
    }
    panic!("no definition of `{}` in:\n{}", name, source_c);
}

fn counts(body: &str) -> (usize, usize, usize) {
    (
        body.matches("_oz_alloc()").count(),
        body.matches("oz_static_retain(").count(),
        body.matches("oz_static_release(").count(),
    )
}

fn check(shape: &Shape, body_src: &str) {
    let src = program(body_src);
    let out = oz_static::transpile(&src)
        .unwrap_or_else(|d| panic!("{} did not transpile: {:?}", shape.what, d));
    let body = function_body(&out.source_c, shape.func);
    let got = counts(&body);
    match shape.known_defect {
        None => assert_eq!(
            got, shape.expect,
            "{}: expected (alloc, retain, release) {:?}, got {:?}\n{}",
            shape.what, shape.expect, got, body
        ),
        Some(issue) => assert_eq!(
            got, shape.expect,
            "{}: this shape is a KNOWN DEFECT ({}) and was asserted to still emit \
             {:?}, but it emits {:?}. If you fixed it, remove the `known_defect` \
             marker in the same change -- that is what keeps this list honest.\n{}",
            shape.what, issue, shape.expect, got, body
        ),
    }
}

/* ---- strong slots ---------------------------------------------------- */

#[test]
fn strong_slots_retain_what_they_are_given() {
    let shapes = [
        (
            Shape {
                what: "ivar, bare spelling",
                func: "Sink_ivarBare",
                expect: (1, 1, 2),
                known_defect: None,
            },
            "\
@interface Sink : OZObject {
	Thing *_ivar;
}
- (void)ivarBare;
@end
@implementation Sink
- (void)ivarBare
{
	Thing *a = [[Thing alloc] init];

	_ivar = a;
}
@end
",
        ),
        (
            Shape {
                what: "ivar, explicit self-> spelling (#352)",
                func: "Sink2_ivarSelf",
                expect: (1, 1, 2),
                known_defect: None,
            },
            "\
@interface Sink2 : OZObject {
	Thing *_ivar;
}
- (void)ivarSelf;
@end
@implementation Sink2
- (void)ivarSelf
{
	Thing *a = [[Thing alloc] init];

	self->_ivar = a;
}
@end
",
        ),
        (
            Shape {
                what: "owned array element, bare spelling",
                func: "Sink3_arrayBare",
                expect: (1, 1, 2),
                known_defect: None,
            },
            "\
@interface Sink3 : OZObject {
	Thing *_arr[2];
}
- (void)arrayBare;
@end
@implementation Sink3
- (void)arrayBare
{
	Thing *a = [[Thing alloc] init];

	_arr[0] = a;
}
@end
",
        ),
        (
            Shape {
                what: "owned array element, explicit self-> spelling (#360)",
                func: "Sink4_arraySelf",
                expect: (1, 1, 2),
                known_defect: None,
            },
            "\
@interface Sink4 : OZObject {
	Thing *_arr[2];
}
- (void)arraySelf;
@end
@implementation Sink4
- (void)arraySelf
{
	Thing *a = [[Thing alloc] init];

	self->_arr[0] = a;
}
@end
",
        ),
        (
            Shape {
                what: "file-scope global, assigned from a local (#359)",
                func: "globalFromLocal",
                expect: (1, 1, 2),
                known_defect: None,
            },
            "\
void globalFromLocal(void)
{
	Thing *a = [[Thing alloc] init];

	g_global = a;
}
",
        ),
        (
            Shape {
                what: "file-scope global, assigned directly (the singleton shape)",
                func: "globalDirect",
                expect: (1, 0, 1),
                known_defect: None,
            },
            "\
void globalDirect(void)
{
	g_global = [[Thing alloc] init];
}
",
        ),
        (
            Shape {
                what: "static local (#359)",
                func: "staticLocal",
                expect: (1, 0, 1),
                known_defect: None,
            },
            "\
void staticLocal(void)
{
	static Thing *cached;

	cached = [[Thing alloc] init];
}
",
        ),
    ];
    for (shape, body) in &shapes {
        check(shape, body);
    }
}

/* ---- locals and their scopes ---------------------------------------- */

#[test]
fn a_local_is_released_when_its_scope_ends() {
    let shapes = [
        (
            Shape { what: "plain local", func: "plainLocal", expect: (1, 0, 1), known_defect: None },
            "\
void plainLocal(void)
{
	Thing *a = [[Thing alloc] init];

	[a tag];
}
",
        ),
        (
            Shape { what: "loop body local", func: "loopLocal", expect: (1, 0, 1), known_defect: None },
            "\
void loopLocal(void)
{
	int i = 0;

	for (i = 0; i < 2; i++) {
		Thing *a = [[Thing alloc] init];

		[a tag];
	}
}
",
        ),
        (
            Shape { what: "nested scope", func: "nestedScope", expect: (1, 0, 1), known_defect: None },
            "\
void nestedScope(void)
{
	{
		Thing *a = [[Thing alloc] init];

		[a tag];
	}
}
",
        ),
        (
            Shape {
                what: "reassigned local releases the old value",
                func: "reassigned",
                expect: (2, 0, 2),
                known_defect: None,
            },
            "\
void reassigned(void)
{
	Thing *a = [[Thing alloc] init];

	a = [[Thing alloc] init];
	[a tag];
}
",
        ),
    ];
    for (shape, body) in &shapes {
        check(shape, body);
    }
}

/* ---- handing a reference outwards ----------------------------------- */

#[test]
fn what_leaves_a_function_is_accounted_for() {
    let shapes = [
        (
            Shape {
                what: "return the owner",
                func: "returnOwner",
                expect: (1, 0, 0),
                known_defect: None,
            },
            "\
Thing *returnOwner(void)
{
	Thing *a = [[Thing alloc] init];

	return a;
}
",
        ),
        (
            Shape {
                what: "return an alias of the owner (#351)",
                func: "returnAlias",
                expect: (1, 0, 0),
                known_defect: None,
            },
            "\
Thing *returnAlias(void)
{
	Thing *a = [[Thing alloc] init];
	Thing *b = a;

	return b;
}
",
        ),
        (
            Shape {
                what: "return an opaque call's result with an owner live (#351)",
                func: "returnOpaque",
                expect: (1, 1, 1),
                known_defect: None,
            },
            "\
Thing *returnOpaque(void)
{
	Thing *a = [[Thing alloc] init];
	Thing *b = factory();

	[a tag];
	return b;
}
",
        ),
        (
            Shape {
                what: "argument to a plain C function stays borrowed, as in ARC",
                func: "cArgument",
                expect: (1, 0, 1),
                known_defect: None,
            },
            "\
void cArgument(void)
{
	Thing *a = [[Thing alloc] init];

	keep(a);
}
",
        ),
        (
            Shape {
                what: "a unanimous +1 through protocol dispatch is released (#361)",
                func: "throughProtocol",
                expect: (0, 0, 1),
                known_defect: None,
            },
            "\
void throughProtocol(id<Supplier> s)
{
	Thing *t = [s supply];

	[t tag];
}
",
        ),
        (
            Shape {
                what: "a +1 result from a statically known class is released",
                func: "fromKnownClass",
                expect: (0, 0, 1),
                known_defect: None,
            },
            "\
void fromKnownClass(Maker *m)
{
	Thing *t = [m supply];

	[t tag];
}
",
        ),
    ];
    for (shape, body) in &shapes {
        check(shape, body);
    }
}

/* ---- shapes the static bar refuses ---------------------------------- */

/// Some escape routes are closed by refusing the code outright, which is
/// as good as tracking it and is worth pinning: if any of these ever
/// starts being *accepted*, it becomes an untracked sink and this file's
/// coverage is silently wrong.
#[test]
fn the_escapes_that_are_refused_stay_refused() {
    let cases = [
        (
            "a block capturing an owned local",
            "\
void capturing(void)
{
	Thing *a = [[Thing alloc] init];
	int (^peek)(void) = ^int(void) {
		return [a tag];
	};

	peek();
}
",
            "captures",
        ),
        (
            "an owned reference stored into a plain C struct field (#359)",
            "\
struct box { struct Thing *held; };
static struct box g_box;

void intoStructField(void)
{
	Thing *a = [[Thing alloc] init];

	g_box.held = a;
}
",
            "C struct field",
        ),
        (
            "a protocol whose implementors disagree about ownership (#361)",
            "\
@protocol TwoMinded
- (Thing *)supply;
@end

@interface Keeper : OZObject {
	Thing *_held;
}
@end
@implementation Keeper
- (Thing *)supply
{
	return _held;
}
@end

@interface Freshener : OZObject
@end
@implementation Freshener
- (Thing *)supply
{
	return [[Thing alloc] init];
}
@end

void throughTwoMinded(id<TwoMinded> s)
{
	Thing *t = [s supply];

	[t tag];
}
",
            "disagree about ownership",
        ),
        (
            "a subclass override that disagrees with its superclass (#365)",
            "\
@interface Sup : OZObject
- (Thing *)give;
@end
@implementation Sup
- (Thing *)give
{
	return [[Thing alloc] init];
}
@end

@interface Sub : Sup {
	Thing *_kept;
}
@end
@implementation Sub
- (Thing *)give
{
	return _kept;
}
@end

void throughSuperclassType(Sup *s)
{
	Thing *t = [s give];

	[t tag];
}
",
            "disagree about ownership",
        ),
        (
            "an owned reference stored into another object's ivar (#359)",
            "\
@interface Pair : OZObject {
	Thing *_held;
}
- (void)fill:(Pair *)other;
@end
@implementation Pair
- (void)fill:(Pair *)other
{
	Thing *a = [[Thing alloc] init];

	other->_held = a;
}
@end
",
            "another object's ivar",
        ),
    ];
    for (what, body, needle) in &cases {
        let diags = common::expect_reject(&program(body));
        assert!(
            diags.contains(needle),
            "{}: expected a located refusal mentioning {:?}, got:\n{}",
            what,
            needle,
            diags
        );
    }
}
