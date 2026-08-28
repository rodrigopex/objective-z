// SPDX-License-Identifier: Apache-2.0
//
// behavior_enum.rs - OZ-092: port of the Python-pipeline "enum" behavior
// category (tests/behavior/cases/enum/) to oz_static.
//
// Each Python fixture pairs an X.m (ObjC declarations only) with a
// hand-written X_test.c (Unity TEST_ASSERT_* calls against the
// Python-generated API). Here each is ported to a single self-contained
// source using the real `OZObject` (`common::OZOBJECT_SRC`) as the root
// class, with a `main()` that printf's the values the original Unity
// asserts checked, and the Rust test asserts the exact stdout.
//
// All three Python fixtures declare their enum directly in the .m file
// (none actually `#import` a separate companion header despite
// `enum_from_header.m`'s name), so all three port over as one
// translation unit with no adaptation needed for oz_static's
// single-file-per-compile limitation.

mod common;
use common::{compile_and_run, OZOBJECT_SRC as PREAMBLE};

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
        PREAMBLE
    );
    let stdout = compile_and_run(&src, "enum_as_ivar");
    assert_eq!(stdout, "dir1=2\ndir2=0\n");
}

/// Ported from tests/behavior/cases/enum/enum_from_header.m +
/// enum_from_header_test.c: an enum constant used in a method-body
/// comparison. The original fixture's `-isHighPriority` returns `BOOL`;
/// ported here as `int` (0/1) predating `BOOL`'s availability (see
/// `common::OZOBJECT_SRC`) -- the behavior under test (comparing an ivar
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
        PREAMBLE
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
        PREAMBLE
    );
    let stdout = compile_and_run(&src, "enum_in_switch");
    assert_eq!(stdout, "red=10\ngreen=20\nblue=30\n");
}
