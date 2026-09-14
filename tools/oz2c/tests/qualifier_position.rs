// SPDX-License-Identifier: Apache-2.0
//
// qualifier_position.rs -- an ARC ownership qualifier belongs to the
// declaration that spells it, and not to a declaration that merely
// contains the token somewhere.
//
// Five sites read `__unsafe_unretained` with
// `node_text(decl).contains("__unsafe_unretained")` over the whole
// declaration, initialiser included. Forty lines from one of them,
// `emit::is_static_declaration` reads the `storage_class_specifier` **node**
// precisely so that it "sees only the storage class and not a `static`
// appearing anywhere else in the text". The ownership qualifier never got
// the same treatment.
//
// One token in a cast is enough:
//
//     Foo *a = (__unsafe_unretained Foo *)[Foo make];
//
// `a` is `__strong` -- the cast qualifies the cast's type, not the
// declaration -- so ARC releases it at scope exit. The substring search saw
// the token, dropped `a` from the managed set, and emitted no release. The
// control differing by that one token emits `oz_release`.
//
// **Two of the four are demonstrable here, and two are not.** The tests that
// fail against the substring search are `a_qualifier_in_a_cast_...` and
// `every_spelling_...`, which reach `owned_locals_of` and
// `managed_object_locals`. `retained_bindings` produces byte-identical C
// either way in every shape tried -- the binding it would retain is elided,
// because the owner outlives the borrow in the same scope -- and
// `static_object_locals` is masked by a separate defect (see
// `a_static_local_is_a_slot_the_store_manages`). Both are changed for
// consistency, and that they are unproven is recorded rather than left to
// be discovered.
//
// Four of the five sites are the hole. The fifth, `collect.rs`'s ivar scan,
// is backstopped: `model::owned_object_ivar_names` gives Clang's AST answer
// precedence and `continue`s before `unretained_ivars` is consulted
// (`model.rs:367`), so on any path with an AST the ivar spelling was already
// safe. The four in `emit.rs` are about *locals* and have no such backstop.
//
// The qualifier reaches a variable from three source positions, and
// tree-sitter puts the `type_qualifier` node in three different parents:
// a direct child of the `declaration`, a child of the `pointer_declarator`
// inside the `init_declarator`, and -- for the cast -- inside the
// `init_declarator`'s *value*. What separates the last is not its depth but
// which side of the `=` it falls on, which is what
// `collect::declares_qualifier` keys on.

mod common;
use common::{compile_and_run_strict, ozobject_src};

const PRELUDE: &str = "\
#include <stdio.h>

int g_freed = 0;

@interface Foo : OZObject
+ (Foo *)make;
- (int)tag;
@end
@implementation Foo
+ (Foo *)make
{
	return [[Foo alloc] init];
}
- (int)tag
{
	return 1;
}
- (void)dealloc
{
	g_freed++;
}
@end
";

fn program(body: &str) -> String {
    format!("/* oz-pool: Foo=4 */\n{}{}\n{}", ozobject_src(), PRELUDE, body)
}

/// The leak, with a runtime oracle rather than a shape assertion.
///
/// `owned_locals_of` is the site: it decides the scope-exit release. The
/// count is what makes this more than "the C looks right" -- `freed=1` can
/// only happen if a release was emitted *and* ran.
#[test]
fn a_qualifier_in_a_cast_does_not_unmanage_the_declaration() {
    let src = program(
        "\
static void scope(void)
{
	Foo *a = (__unsafe_unretained Foo *)[Foo make];

	printf(\"tag=%d\\n\", [a tag]);
}

int main(void)
{
	scope();
	printf(\"freed=%d\\n\", g_freed);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "qualpos_cast_does_not_unmanage"),
        "tag=1\nfreed=1\n",
        "`a` is __strong -- the cast qualifies the cast's type, not the declaration -- so the \
         scope owes it a release. A substring search over the declaration sees the token in \
         the cast and emits none, which leaks: freed=0"
    );
}

/// The other direction, which is what makes the fix a narrowing rather than
/// a removal: a genuinely `__unsafe_unretained` local is still a borrow and
/// still must not be released.
///
/// A control as well -- it passes either way, since the declaration really
/// does spell the qualifier and both readings agree. It is here so that a
/// future change cannot fix the position question by dropping the qualifier
/// altogether.
///
/// Kept in one program with the owner, because the property is the
/// *difference* between them: one release for two names holding the same
/// object. Releasing the borrow too would be the double free the qualifier
/// exists to prevent, and `freed` would still read 1 at the print -- so the
/// oracle is `tag` afterwards, which reads through a pointer the borrow
/// would have freed.
#[test]
fn a_declared_unretained_local_is_still_a_borrow() {
    let src = program(
        "\
static void scope(void)
{
	Foo *owner = [Foo make];
	__unsafe_unretained Foo *borrow = owner;

	printf(\"same=%d tag=%d\\n\", borrow == owner, [borrow tag]);
	printf(\"freed inside=%d\\n\", g_freed);
}

int main(void)
{
	scope();
	printf(\"freed=%d\\n\", g_freed);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "qualpos_unretained_still_borrow"),
        "same=1 tag=1\nfreed inside=0\nfreed=1\n",
        "the borrow is not released, so the one object is freed exactly once at scope exit"
    );
}

/// Every spelling that qualifies the variable, and the one that does not,
/// in one table.
///
/// The three that qualify it put the `type_qualifier` node in two different
/// parents, so a check on one child position would cover some and not
/// others -- the shape of every ARC defect since #351. The fourth row is
/// the cast, and it is the only one whose local is managed.
#[test]
fn every_spelling_that_qualifies_the_variable_is_seen() {
    /* The receiver travels with the row because an `id` local names no
     * class to dispatch to and has to be cast at the send -- ordinary
     * oz2c behaviour, unrelated to the qualifier, and a cast written into
     * every row would put a cast in the one row whose subject is a cast. */
    let cases: &[(&str, &str, &str, bool)] = &[
        ("leading", "__unsafe_unretained Foo *b = owner;", "b", false),
        ("after the star", "Foo *__unsafe_unretained b = owner;", "b", false),
        ("after the specifier", "__unsafe_unretained id b = owner;", "(Foo *)b", false),
        /* Declared strong; the qualifier is the cast's. This is the one
         * that must be managed, and the one that was not. */
        ("in a cast", "Foo *b = (__unsafe_unretained Foo *)[Foo make];", "b", true),
    ];
    for (name, decl, receiver, managed) in cases {
        let src = program(&format!(
            "\
static void scope(void)
{{
	Foo *owner = [Foo make];

	{}
	printf(\"tag=%d\\n\", [{} tag]);
	printf(\"owner=%d\\n\", owner != 0);
}}

int main(void)
{{
	scope();
	printf(\"freed=%d\\n\", g_freed);
	return 0;
}}
",
            decl, receiver
        ));
        /* A borrow leaves one object, so one dealloc. The cast row
         * allocates a second and owns it, so two. */
        let want = if *managed {
            "tag=1\nowner=1\nfreed=2\n"
        } else {
            "tag=1\nowner=1\nfreed=1\n"
        };
        assert_eq!(
            compile_and_run_strict(&src, &format!("qualpos_spelling_{}", name.replace(' ', "_"))),
            want,
            "the '{}' spelling: expected the local to be {}",
            name,
            if *managed { "managed" } else { "a borrow" }
        );
    }
}

/// A `static` object local -- **a control, not evidence.**
///
/// It passes with the substring search too, and saying so is the point:
/// the token here is in the *store*, not in the declaration, so neither
/// reading of the declaration sees it. What the case pins is that a
/// `static` local is a strong *slot* rather than a scope-owned reference --
/// the store manages it and the scope owes it nothing, so a scope-exit
/// release here would free the object on the way out of the first call and
/// hand the second a dangling pointer (#359).
///
/// **The `static_object_locals` site's own qualifier check has no test,
/// and cannot have one today.** Reaching it needs the token inside the
/// declaration's initialiser, and a `static` local's initialiser must be a
/// constant expression in C, which leaves only
/// `static Foo *slot = (__unsafe_unretained Foo *)0;`. That shape loses its
/// release-first store for an unrelated reason: measured, a *plain*
/// `(Foo *)0` cast with no qualifier anywhere loses it too, so the cast in
/// the initialiser already unmanages the slot before the qualifier is
/// consulted. One defect masks the other. Filed separately; the fix here
/// is applied to that site for consistency and is unproven there.
#[test]
fn a_static_local_is_a_slot_the_store_manages() {
    let src = program(
        "\
static int cached_tag(void)
{
	static Foo *cache = 0;

	if (cache == 0) {
		cache = (__unsafe_unretained Foo *)[Foo make];
	}
	return [cache tag];
}

int main(void)
{
	printf(\"a=%d b=%d\\n\", cached_tag(), cached_tag());
	printf(\"freed=%d\\n\", g_freed);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "qualpos_static_local"),
        "a=1 b=1\nfreed=0\n",
        "the static slot outlives the call, so the second call reads the same live object and \
         nothing has been freed -- a scope-exit release here would free it on the way out of \
         the first call and hand the second a dangling pointer (#359)"
    );
}

/// The qualifier still has to be spelled exactly, not merely contained.
///
/// Also a control: the old substring search happened to get this right
/// too, because `contains` was applied to a declaration that did not hold
/// the neighbouring variable. Kept because the fix is a `trim()`-and-compare
/// on whole node text, and a future rewrite reaching for `starts_with`
/// would break it.
///
/// `__unsafe_unretainedly` is not a qualifier, and a check that compared
/// substrings rather than whole node text would accept it. This is the
/// mirror of the defect -- a false *positive* on the token rather than on
/// its position -- and `declares_qualifier` trims and compares the whole
/// node, so a longer identifier is a different token.
#[test]
fn a_longer_identifier_is_not_the_qualifier() {
    let src = program(
        "\
static void scope(void)
{
	int __unsafe_unretainedly = 1;
	Foo *a = [Foo make];

	printf(\"tag=%d n=%d\\n\", [a tag], __unsafe_unretainedly);
}

int main(void)
{
	scope();
	printf(\"freed=%d\\n\", g_freed);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "qualpos_longer_identifier"),
        "tag=1 n=1\nfreed=1\n",
        "a variable whose *name* contains the qualifier does not unmanage a declaration \
         beside it"
    );
}
/// C scopes the two qualifier positions differently, and the declaration is
/// the wrong granularity for the second.
///
/// ```objc
/// __unsafe_unretained Foo *a, *b;   /* both unretained */
/// Foo *__unsafe_unretained a, *b;   /* `a` unretained, `b` __strong */
/// ```
///
/// A qualifier among the declaration's own children introduces every
/// declarator; one inside a declarator applies to that declarator alone.
/// Answering for the whole declaration made `b` a borrow, and `b` holds a
/// `+1` from its own initialiser -- so nothing released it. One object freed
/// where ARC frees two.
///
/// **This half was pre-existing and reading the node does not fix it.** The
/// substring search and a per-declaration node check give the same wrong
/// answer here, which is why `collect::qualifies` takes the declarator as
/// well: "read the node instead of the text" is necessary and not
/// sufficient.
#[test]
fn a_qualifier_after_the_star_binds_to_its_own_declarator() {
    let src = program(
        "\
static void scope(void)
{
	Foo *owner = [Foo make];
	Foo *__unsafe_unretained a = owner, *b = [Foo make];

	printf(\"a=%d b=%d\\n\", a == owner, b != owner);
}

int main(void)
{
	scope();
	printf(\"freed=%d\\n\", g_freed);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "qualpos_after_star_one_declarator"),
        "a=1 b=1\nfreed=2\n",
        "`a` is the borrow and `b` is strong, so both objects are freed exactly once: the \
         owner through `owner`, and `b`'s own allocation through `b`. Treating the whole \
         declaration as unretained leaks `b`: freed=1"
    );
}
