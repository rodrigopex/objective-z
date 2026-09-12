// init_prefix_selectors.rs -- a selector that merely begins with `init`
// is not an initialiser (#398).
//
// Five places in `arc.rs` used to ask `selector.starts_with("init")`,
// which matches every ordinary method whose name happens to start with
// those four letters. Three of the five then treated the receiver's `+1`
// as handed back through the return value, so for
// `[[Thing alloc] initialValue]` the *`int`* became the reference:
// `oz_static_release` was passed the integer and dereferenced it.
//
// The tests that matter here **run** the program. The defect's signature
// is a segfault, not a wrong string, and it was invisible to every check
// that read the generated C without compiling it.

mod common;

use common::{compile_and_run, ozobject_src};

fn program(decls: &str, body: &str) -> String {
        format!("{}{}\n#include <stdio.h>\n{}", ozobject_src(), decls, body)
}

/// The issue's own reproduction.
///
/// Without the fix this prints `before` and exits on signal 11 --
/// `oz_static_release` is handed the integer 42 as an object pointer.
#[test]
fn an_int_returning_init_prefixed_selector_is_not_an_initialiser() {
        let src = program(
                "\
@interface Thing : OZObject
- (int)initialValue;
@end
@implementation Thing
- (int)initialValue
{
\treturn 42;
}
@end
",
                "\
int main(void) {
\tprintf(\"before\\n\");
\tprintf(\"v=%d\\n\", [[Thing alloc] initialValue]);
\tprintf(\"after\\n\");
\treturn 0;
}
",
        );
        let out = compile_and_run(&src, "an_int_returning_init_prefixed_selector_is_not_an_initialiser");
        assert_eq!(out, "before\nv=42\nafter\n");
}

/// Every spelling the issue lists, each sent to a **fresh allocation**.
///
/// The receiver has to be an owning expression for the defect to reach
/// it: sent to a bound local the `+1` never flows through the send, and
/// the first draft of this test did exactly that and passed with the fix
/// reverted -- vacuous, for the same reason as the loop-escape test that
/// sat in `main()` where the static bar never looks. `[[Thing alloc]
/// initialCount]` is the shape that bites.
///
/// Pinned across return types rather than for `int` alone: `BOOL` and
/// `void` are the two a "does it return something object-shaped" test
/// could plausibly get wrong in different ways.
#[test]
fn the_whole_init_prefixed_family_is_left_alone() {
        let src = program(
                "\
@interface Thing : OZObject
- (int)initialValue;
- (int)initialCount;
- (BOOL)initialised;
- (void)initializeCache;
- (int)initialState;
@end
@implementation Thing
- (int)initialValue { return 1; }
- (int)initialCount { return 2; }
- (BOOL)initialised { return YES; }
- (void)initializeCache { }
- (int)initialState { return 3; }
@end
",
                "\
int main(void) {
\tprintf(\"%d\\n\", [[Thing alloc] initialValue]);
\tprintf(\"%d\\n\", [[Thing alloc] initialCount]);
\tprintf(\"%d\\n\", (int)[[Thing alloc] initialised]);
\tprintf(\"%d\\n\", [[Thing alloc] initialState]);
\t[[Thing alloc] initializeCache];
\tprintf(\"survived\\n\");
\treturn 0;
}
",
        );
        let out = compile_and_run(&src, "the_whole_init_prefixed_family_is_left_alone");
        assert_eq!(out, "1\n2\n1\n3\nsurvived\n");
}

/// The other direction, which is the one a careless fix breaks: a real
/// initialiser must still consume the `alloc`'s `+1` and hand it back, so
/// `alloc`/`initWith...` remains one reference and `-dealloc` runs once.
///
/// If `is_initialiser` answered `false` here, the `alloc` temporary would
/// be accounted separately from the initialised object and the program
/// would either leak it or release it twice.
#[test]
fn a_real_init_with_selector_still_consumes_the_allocation() {
        let src = program(
                "\
@interface Sensor : OZObject
{
\tint _v;
}
- (instancetype)initWithValue:(int)v;
- (int)value;
@end
@implementation Sensor
- (instancetype)initWithValue:(int)v
{
\t_v = v;
\treturn self;
}
- (int)value
{
\treturn _v;
}
- (void)dealloc
{
\tprintf(\"dealloc %d\\n\", _v);
}
@end
",
                "\
int main(void) {
\t/* Braced so the dealloc is ordered before `done`, which a hand
\t * release used to do (#428). */
\t{
\t\tSensor *s = [[Sensor alloc] initWithValue:7];
\t\tprintf(\"v=%d rc=%d\\n\", [s value], [s retainCount]);
\t}
\tprintf(\"done\\n\");
\treturn 0;
}
",
        );
        let out = compile_and_run(&src, "a_real_init_with_selector_still_consumes_the_allocation");
        assert_eq!(out, "v=7 rc=1\ndealloc 7\ndone\n");
}

/// `-init` itself, on a class that declares none of its own, must keep
/// pairing with `alloc` -- that pairing is what `is_initialiser` special-
/// cases, and getting it wrong would double-account every object in every
/// program.
#[test]
fn plain_init_still_pairs_with_alloc() {
        let src = program(
                "\
@interface Plain : OZObject
@end
@implementation Plain
- (void)dealloc
{
\tprintf(\"dealloc\\n\");
}
@end
",
                "\
int main(void) {
\t{
\t\tPlain *p = [[Plain alloc] init];
\t\tprintf(\"rc=%d\\n\", [p retainCount]);
\t}
\tprintf(\"done\\n\");
\treturn 0;
}
",
        );
        let out = compile_and_run(&src, "plain_init_still_pairs_with_alloc");
        assert_eq!(out, "rc=1\ndealloc\ndone\n");
}
