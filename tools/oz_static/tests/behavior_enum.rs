// SPDX-License-Identifier: Apache-2.0
//
// behavior_enum.rs - OZ-092: port of the Python-pipeline "enum" behavior
// category (tests/behavior/cases/enum/) to oz_static.
//
// Each Python fixture pairs an X.m (ObjC declarations only) with a
// hand-written X_test.c (Unity TEST_ASSERT_* calls against the
// Python-generated API). Here each is ported to a single self-contained
// source using the real `OZObject` (`common::ozobject_src`) as the root
// class, with a `main()` that printf's the values the original Unity
// asserts checked, and the Rust test asserts the exact stdout.
//
// All three Python fixtures declare their enum directly in the .m file
// (none actually `#import` a separate companion header despite
// `enum_from_header.m`'s name), so all three port over as one
// translation unit with no adaptation needed for oz_static's
// single-file-per-compile limitation.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// Ported from tests/behavior/cases/enum/enum_as_ivar.m +
/// enum_as_ivar_test.c: an enum used as an ivar's type, set through a
/// setter and read back through a getter.
#[test]
fn enum_as_ivar() {
    let src = format!(
        "{}
enum Direction {{
	DirectionNorth = 0,
	DirectionSouth = 1,
	DirectionEast = 2,
	DirectionWest = 3
}};

@interface EnumIvarTest : OZObject {{
	enum Direction _dir;
}}
- (void)setDirection:(enum Direction)d;
- (enum Direction)direction;
@end

@implementation EnumIvarTest
- (void)setDirection:(enum Direction)d {{
	_dir = d;
}}
- (enum Direction)direction {{
	return _dir;
}}
@end

#include <stdio.h>

int main(void) {{
	EnumIvarTest *t = [EnumIvarTest alloc];
	[t setDirection:2];
	printf(\"dir1=%d\\n\", [t direction]);
	[t setDirection:0];
	printf(\"dir2=%d\\n\", [t direction]);
	[t release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "enum_as_ivar");
    assert_eq!(stdout, "dir1=2\ndir2=0\n");
}

/// Ported from tests/behavior/cases/enum/enum_from_header.m +
/// enum_from_header_test.c: an enum constant used in a method-body
/// comparison. The original fixture's `-isHighPriority` returns `BOOL`;
/// ported here as `int` (0/1) predating `BOOL`'s availability (see
/// `common::ozobject_src`) -- the behavior under test (comparing an ivar
/// against an enum constant) is unaffected by that substitution.
#[test]
fn enum_constant_comparison() {
    let src = format!(
        "{}
enum Priority {{
	PriorityLow = 1,
	PriorityMedium = 5,
	PriorityHigh = 10
}};

@interface EnumHeaderTest : OZObject {{
	enum Priority _prio;
}}
- (void)setPriority:(enum Priority)p;
- (int)isHighPriority;
@end

@implementation EnumHeaderTest
- (void)setPriority:(enum Priority)p {{
	_prio = p;
}}
- (int)isHighPriority {{
	return _prio >= PriorityHigh;
}}
@end

#include <stdio.h>

int main(void) {{
	EnumHeaderTest *hi = [EnumHeaderTest alloc];
	[hi setPriority:10];
	printf(\"high=%d\\n\", [hi isHighPriority]);
	[hi release];

	EnumHeaderTest *lo = [EnumHeaderTest alloc];
	[lo setPriority:1];
	printf(\"low=%d\\n\", [lo isHighPriority]);
	[lo release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "enum_constant_comparison");
    assert_eq!(stdout, "high=1\nlow=0\n");
}

/// Ported from tests/behavior/cases/enum/enum_in_switch.m +
/// enum_in_switch_test.c: an enum-typed parameter dispatched through a
/// switch/case over its constants.
#[test]
fn enum_in_switch() {
    let src = format!(
        "{}
enum Color {{
	ColorRed = 0,
	ColorGreen = 1,
	ColorBlue = 2
}};

@interface EnumSwitchTest : OZObject {{
	int _result;
}}
- (void)classifyColor:(enum Color)c;
- (int)result;
@end

@implementation EnumSwitchTest
- (void)classifyColor:(enum Color)c {{
	switch (c) {{
	case ColorRed:
		_result = 10;
		break;
	case ColorGreen:
		_result = 20;
		break;
	case ColorBlue:
		_result = 30;
		break;
	}}
}}
- (int)result {{
	return _result;
}}
@end

#include <stdio.h>

int main(void) {{
	EnumSwitchTest *red = [EnumSwitchTest alloc];
	[red classifyColor:0];
	printf(\"red=%d\\n\", [red result]);
	[red release];

	EnumSwitchTest *green = [EnumSwitchTest alloc];
	[green classifyColor:1];
	printf(\"green=%d\\n\", [green result]);
	[green release];

	EnumSwitchTest *blue = [EnumSwitchTest alloc];
	[blue classifyColor:2];
	printf(\"blue=%d\\n\", [blue result]);
	[blue release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "enum_in_switch");
    assert_eq!(stdout, "red=10\ngreen=20\nblue=30\n");
}

/// Not a port -- the oracle's own `tests/behavior/cases/enum/` has no
/// anonymous-enum case, so there is nothing to port. This is new
/// coverage for a construct reportedly unsupported: `enum { ... };` with
/// no tag name.
///
/// It already works, and needed no code change: the enum-hoisting logic
/// (`emit.rs`'s `enum_specifier` arm) moves the whole definition's text
/// to the companion header verbatim regardless of whether it has a tag,
/// and an anonymous enum's enumerators are ordinary identifiers usable
/// anywhere an `int` constant is, tag or no tag -- the same as in real C.
///
/// A separate, narrower construct -- an anonymous aggregate used *inline
/// as a method's value type*, e.g. `- (enum { A, B })foo;` -- used to
/// degrade to the bare word `enum` and is now rejected outright by
/// `collect::reject_inline_anonymous_aggregates`; see
/// `inline_anonymous_enum_as_return_type_rejected` and friends below.
/// That shape is unrelated to the one exercised here: this test's enum is
/// defined once at file scope and every use of it is by its *enumerator
/// constants* (plain `int`s), never by naming the anonymous type itself.
#[test]
fn anonymous_enum_constants_usable() {
    let src = format!(
        "{}
enum {{
	AnonRed,
	AnonGreen,
	AnonBlue
}};

@interface AnonEnumTest : OZObject
- (int)pick;
@end

@implementation AnonEnumTest
- (int)pick {{
	return AnonGreen;
}}
@end

#include <stdio.h>

int main(void) {{
	AnonEnumTest *t = [AnonEnumTest alloc];
	printf(\"red=%d\\n\", AnonRed);
	printf(\"pick=%d\\n\", [t pick]);
	printf(\"blue=%d\\n\", AnonBlue);
	[t release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "anonymous_enum_constants_usable");
    assert_eq!(stdout, "red=0\npick=1\nblue=2\n");
}

// ---------------------------------------------------------------------------
// Inline anonymous aggregates as method value types -- rejected
// ---------------------------------------------------------------------------
//
// New coverage, no oracle counterpart: every case in
// tests/behavior/cases/enum/ declares a *named* file-scope enum and refers
// to it by tag, and the oracle's own `_collect_enum_def` keys on the enum's
// name, so an untagged one has no defined behavior there either.
//
// These shapes used to be accepted and emit broken C -- `enum Foo_ret(struct
// Foo *self)` for a return type, `enum v` for a parameter -- while an inline
// anonymous *union* was worse still, silently lowering to its first member's
// type (`int u`), which compiles and quietly passes the wrong type. All are
// hard, located errors now.

/// `- (enum { A, B })sel;` -- used to emit `enum Foo_ret(struct Foo *self)`.
#[test]
fn inline_anonymous_enum_as_return_type_rejected() {
    let src = format!(
        "{}
@interface AnonRet : OZObject
- (enum {{ RET_A, RET_B }})ret;
@end
@implementation AnonRet
- (enum {{ RET_A, RET_B }})ret {{
	return RET_A;
}}
@end
",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("inline anonymous 'enum'"), "diagnostics: {}", diags);
    assert!(diags.contains("enum Tag { ... }"), "diagnostics: {}", diags);
}

/// `- (void)take:(enum { A, B })v;` -- used to emit a parameter typed `enum v`.
#[test]
fn inline_anonymous_enum_as_parameter_rejected() {
    let src = format!(
        "{}
@interface AnonParam : OZObject
- (void)take:(enum {{ PAR_A, PAR_B }})v;
@end
@implementation AnonParam
- (void)take:(enum {{ PAR_A, PAR_B }})v {{
	(void)v;
}}
@end
",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("inline anonymous 'enum'"), "diagnostics: {}", diags);
}

#[test]
fn inline_anonymous_struct_as_parameter_rejected() {
    let src = format!(
        "{}
@interface AnonStruct : OZObject
- (void)takePt:(struct {{ int x; }})p;
@end
@implementation AnonStruct
- (void)takePt:(struct {{ int x; }})p {{
	(void)p;
}}
@end
",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("inline anonymous 'struct'"), "diagnostics: {}", diags);
    assert!(diags.contains("struct Tag { ... }"), "diagnostics: {}", diags);
}

/// The worst of the family: this one used to *compile*, with the parameter
/// silently typed `int` (the union's first member) instead of the union.
#[test]
fn inline_anonymous_union_as_parameter_rejected() {
    let src = format!(
        "{}
@interface AnonUnion : OZObject
- (void)takeU:(union {{ int a; float b; }})u;
@end
@implementation AnonUnion
- (void)takeU:(union {{ int a; float b; }})u {{
	(void)u;
}}
@end
",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("inline anonymous 'union'"), "diagnostics: {}", diags);
}

/// The accepted counterpart, and the shape the rejection message points at:
/// a named file-scope enum referred to by tag in both positions. This is
/// what every oracle enum case does.
#[test]
fn named_file_scope_enum_as_return_and_parameter_accepted() {
    let src = format!(
        "{}
enum Level {{
	LevelLow = 1,
	LevelHigh = 9
}};

@interface NamedLevel : OZObject {{
	enum Level _level;
}}
- (void)setLevel:(enum Level)l;
- (enum Level)level;
@end

@implementation NamedLevel
- (void)setLevel:(enum Level)l {{
	_level = l;
}}
- (enum Level)level {{
	return _level;
}}
@end

#include <stdio.h>

int main(void) {{
	NamedLevel *n = [NamedLevel alloc];
	[n setLevel:LevelHigh];
	printf(\"level=%d\\n\", (int)[n level]);
	[n setLevel:LevelLow];
	printf(\"level2=%d\\n\", (int)[n level]);
	[n release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "named_file_scope_enum_as_return_and_parameter");
    assert_eq!(stdout, "level=9\nlevel2=1\n");
}

/// An anonymous aggregate is still fine as an *ivar*: nothing needs to name
/// the type, because `emit::lower_ivar_decl` copies the declaration through
/// with its body intact. Pins the boundary the rejection is scoped to, so a
/// future broadening of it fails here rather than silently.
#[test]
fn inline_anonymous_enum_as_ivar_still_accepted() {
    let src = format!(
        "{}
@interface AnonIvar : OZObject {{
	enum {{ ModeIdle, ModeBusy }} _mode;
	struct {{ int x; }} _pt;
}}
- (void)setup;
- (int)mode;
- (int)px;
@end

@implementation AnonIvar
- (void)setup {{
	_mode = ModeBusy;
	_pt.x = 42;
}}
- (int)mode {{
	return (int)_mode;
}}
- (int)px {{
	return _pt.x;
}}
@end

#include <stdio.h>

int main(void) {{
	AnonIvar *a = [AnonIvar alloc];
	[a setup];
	printf(\"mode=%d\\n\", [a mode]);
	printf(\"px=%d\\n\", [a px]);
	[a release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "inline_anonymous_enum_as_ivar_still_accepted");
    assert_eq!(stdout, "mode=1\npx=42\n");
}
