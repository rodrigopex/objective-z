// SPDX-License-Identifier: Apache-2.0
//
// weak_every_position.rs -- `__weak` is refused wherever it is written, and
// the remedy the refusal names is accepted in the same place (#448).
//
// `__weak` is prohibited by design: nothing can zero a weak reference
// without a runtime, so it would behave as an unretained strong reference,
// silently, which is the exact bug the qualifier exists to prevent. It is
// deliberately absent from `emit::STRIPPED_ARC_QUALIFIERS` so that it fails
// rather than being quietly dropped.
//
// **It failed in one position out of ten.** Measured on the tree before the
// fix, `__weak` reached the generated C verbatim from a local, a `static`
// local, a file-scope declaration, a `for`-header declaration, a method
// parameter, a plain-C-function parameter, a block parameter, a C struct
// field, and the *type* of a property. Only an ivar was refused. The
// failure was then the C compiler's, on a file the author never wrote, with
// no location pointing at the `.m` -- and on macOS not even that, since
// Apple clang accepts ARC qualifiers in plain C where Linux GCC rejects
// them (docs/STATUS.md, #428), so every host gate could pass over output
// that only CI or a target build refuses.
//
// **The issue reporting this said two positions were covered, an ivar and a
// property, and the property half does not survive measurement.** What
// `collect.rs` refuses is the property *attribute* `weak`
// (`@property (weak) Thing *w;`). The *type qualifier* on the same property
// (`@property () __weak Thing *w;`) is a different spelling, is not a
// `type_qualifier` under the property attribute list at all, and reached
// the output. Two spellings, one refused and one not, read as one position
// -- so the count of covered positions was one, not two, and the count of
// holes was nine, not six. `the_two_property_spellings_are_different_rules`
// pins the distinction so it cannot be collapsed again.

mod common;
use common::ozobject_src;

/// Every position a `type_qualifier` can reach, as `(name, source)`.
///
/// Each body is a whole translation unit, because `staticbar` scans from
/// the root and several of these positions are outside any method body --
/// which is the reason the check is a whole-tree walk rather than an
/// addition to a body-scoped entry point.
const POSITIONS: &[(&str, &str)] = &[
    (
        "ivar",
        "\
@interface T : OZObject {
	QUAL T *_w;
}
@end
@implementation T
@end
int main(void) { return 0; }
",
    ),
    (
        "property type",
        "\
@interface T : OZObject
@property (nonatomic) QUAL T *w;
@end
@implementation T
@end
int main(void) { return 0; }
",
    ),
    (
        "local",
        "\
@interface T : OZObject
- (void)poke;
@end
@implementation T
- (void)poke
{
	QUAL T *t = self;

	(void)t;
}
@end
int main(void) { return 0; }
",
    ),
    (
        "static local",
        "\
@interface T : OZObject
- (void)poke;
@end
@implementation T
- (void)poke
{
	static QUAL T *t = 0;

	(void)t;
}
@end
int main(void) { return 0; }
",
    ),
    (
        "file scope",
        "\
@interface T : OZObject
@end
@implementation T
@end
static QUAL T *g_t = 0;
int main(void) { (void)g_t; return 0; }
",
    ),
    (
        "method parameter",
        "\
@interface T : OZObject
- (void)take:(QUAL T *)x;
@end
@implementation T
- (void)take:(QUAL T *)x
{
	(void)x;
}
@end
int main(void) { return 0; }
",
    ),
    (
        "C function parameter",
        "\
@interface T : OZObject
@end
@implementation T
@end
static void take(QUAL T *x)
{
	(void)x;
}
int main(void) { take(0); return 0; }
",
    ),
    (
        "for-header declaration",
        "\
@interface T : OZObject
- (void)poke;
@end
@implementation T
- (void)poke
{
	for (QUAL T *t = self; t != 0; t = 0) {
		(void)t;
	}
}
@end
int main(void) { return 0; }
",
    ),
    (
        "C struct field",
        "\
@interface T : OZObject
@end
@implementation T
@end
struct Holder {
	QUAL T *w;
};
int main(void) { struct Holder h; (void)h; return 0; }
",
    ),
    (
        "block parameter",
        "\
@interface T : OZObject
- (void)poke;
@end
@implementation T
- (void)poke
{
	void (^b)(QUAL T *) = ^(QUAL T *x) {
		(void)x;
	};

	(void)b;
}
@end
int main(void) { return 0; }
",
    ),
];

fn program(body: &str, qualifier: &str) -> String {
    format!("{}{}", ozobject_src(), body.replace("QUAL", qualifier))
}

/// All ten positions, refused, with a location.
///
/// The location is asserted rather than assumed, because it is the whole
/// point: nine of these already failed *somewhere* -- in the C compiler, on
/// generated C -- and what the author could not get was a line in their own
/// `.m`. A refusal with no location would satisfy "it errors" and none of
/// the reason the issue exists.
#[test]
fn weak_is_refused_in_every_position() {
    for (name, body) in POSITIONS {
        let diags = common::expect_reject(&program(body, "__weak"));
        assert!(
            diags.contains("'__weak' is not supported"),
            "the '{}' position must be refused by oz2c's own diagnostic; got:\n{}",
            name,
            diags
        );
        assert!(
            diags.lines().any(|l| {
                let mut parts = l.trim_start().splitn(3, ':');
                matches!(
                    (parts.next(), parts.next()),
                    (Some(a), Some(b))
                        if !a.is_empty()
                            && a.chars().all(|c| c.is_ascii_digit())
                            && !b.is_empty()
                            && b.chars().all(|c| c.is_ascii_digit())
                )
            }),
            "the '{}' refusal must carry a line:column in the author's own source, which is \
             what nine of these positions could not give -- they failed in the C compiler on \
             generated C; got:\n{}",
            name,
            diags
        );
    }
}

/// The remedy is accepted in every position it is offered in.
///
/// This is the assertion that makes the advice more than a sentence, and it
/// is #425's standing bar: a remedy has to be one the author can write
/// **and** the checker will honour. The diagnostic says "use
/// `__unsafe_unretained` and clear it explicitly" in all ten positions, so
/// all ten have to accept it -- otherwise the refusal sends the author
/// somewhere that refuses them too, which is the defect #425 was.
#[test]
fn the_remedy_is_accepted_everywhere_it_is_offered() {
    for (name, body) in POSITIONS {
        let src = program(body, "__unsafe_unretained");
        assert!(
            oz2c::transpile(&src).is_ok(),
            "the '{}' position refuses '__weak' and names '__unsafe_unretained' as the fix, \
             so it must accept it; got:\n{}",
            name,
            oz2c::transpile(&src)
                .err()
                .map(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default()
        );
    }
}

/// A weak **property attribute** and a `__weak` **type qualifier** are two
/// rules, not one, and conflating them is what made #448 undercount.
///
/// `@property (weak) T *w;` is refused by `collect.rs`'s attribute parse --
/// the token is an identifier in the property's attribute list, not a
/// `type_qualifier` node, so the whole-tree qualifier walk cannot see it and
/// must not be expected to. `@property () __weak T *w;` is the qualifier,
/// and it is the one that reached the generated C while the issue recorded
/// the position as covered.
///
/// Pinned as a pair so that a future change cannot delete one refusal on
/// the grounds that the other covers it.
#[test]
fn the_two_property_spellings_are_different_rules() {
    let attribute = common::expect_reject(&format!(
        "{}{}",
        ozobject_src(),
        "\
@interface T : OZObject
@property (nonatomic, weak) T *w;
@end
@implementation T
@end
int main(void) { return 0; }
"
    ));
    assert!(
        attribute.contains("'weak' property"),
        "the attribute spelling is the property parse's rule; got:\n{}",
        attribute
    );

    let qualifier = common::expect_reject(&format!(
        "{}{}",
        ozobject_src(),
        "\
@interface T : OZObject
@property (nonatomic) __weak T *w;
@end
@implementation T
@end
int main(void) { return 0; }
"
    ));
    assert!(
        qualifier.contains("'__weak' is not supported"),
        "the qualifier spelling is the whole-tree walk's rule; got:\n{}",
        qualifier
    );
    assert!(
        !qualifier.contains("'weak' property"),
        "and it is not reached by the attribute rule -- if it were, the qualifier walk would \
         be redundant and #448 would have been a non-issue; got:\n{}",
        qualifier
    );
}

/// One diagnostic per written token, not per declaration.
///
/// A method declared in the `@interface` and defined in the
/// `@implementation` spells the qualifier twice, and two tokens are two
/// mistakes to fix, so two diagnostics is right. Pinned because the natural
/// way to "tidy" it -- one diagnostic per declaration -- would hide the
/// second occurrence, and the author would fix one and hit the other.
#[test]
fn each_written_token_is_reported() {
    let diags = common::expect_reject(&program(
        POSITIONS.iter().find(|(n, _)| *n == "method parameter").expect("row present").1,
        "__weak",
    ));
    assert_eq!(
        diags.matches("'__weak' is not supported").count(),
        2,
        "the declaration and the definition each spell it, so each is reported; got:\n{}",
        diags
    );
}
