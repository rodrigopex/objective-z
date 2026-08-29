// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_mutable_string.rs - OZ-092 Foundation work:
// OZMutableString, ported from tests/behavior/cases/foundation/
// mutable_string_basic.m (11 assertions -- 8 exercise OZMutableString
// itself, 3 exercise plain OZString methods -- hasPrefix:/hasSuffix:/
// isEqualToString: -- not covered by the earlier OZString port).
//
// Uses the real `OZObject` (`common::ozobject_src`) as the root class and
// the real `OZString` (`common::ozstring_src`) as OZMutableString's
// superclass. OZMutableString itself (`common::ozmutablestring_src`) is a
// full transplant, own `-init*`/`-dealloc` included -- unlike OZArray, it
// needs no oz_static-side special-casing at all: its growable `_data`
// buffer is already malloc-based in the real source (ordinary string-
// growth logic, not the object's own alloc/free machinery), and
// `[super init]`/inherited-ivar access through a base chain are both
// already exercised elsewhere (behavior_dispatch.rs, end_to_end_behavior.rs).

mod common;
use common::{compile_and_run, ozmutablestring_src, ozobject_src as PREAMBLE, ozstring_src};

#[test]
fn mutable_string_basic_all_operations() {
    let src = format!(
        "{}{}{}\n\
@interface MutableStringTest : OZObject {{
	struct OZMutableString *_ms;
}}
- (void)buildFromCString;
- (void)buildFromOZString;
- (void)buildWithCapacity;
- (void)buildAndAppendCString;
- (void)buildAndAppendString;
- (void)buildAndAppendGrow;
- (void)buildAndSetString;
- (void)buildAndSetStringNil;
- (const char *)result;
- (unsigned int)resultLength;
- (BOOL)hasPrefixTrue;
- (BOOL)hasSuffixTrue;
- (BOOL)isEqualToStringTrue;
@end

@implementation MutableStringTest

- (void)buildFromCString {{
	_ms = [[OZMutableString alloc] initWithCString:\"hello\"];
}}

- (void)buildFromOZString {{
	OZString *src = @\"world\";
	_ms = [[OZMutableString alloc] initWithString:src];
}}

- (void)buildWithCapacity {{
	_ms = [[OZMutableString alloc] initWithCapacity:64];
	[_ms appendCString:\"reserved\"];
}}

- (void)buildAndAppendCString {{
	_ms = [[OZMutableString alloc] initWithCString:\"hello\"];
	[_ms appendCString:\", world\"];
}}

- (void)buildAndAppendString {{
	_ms = [[OZMutableString alloc] initWithCString:\"hello\"];
	OZString *suffix = @\", world\";
	[_ms appendString:suffix];
}}

- (void)buildAndAppendGrow {{
	_ms = [[OZMutableString alloc] initWithCString:\"a\"];
	[_ms appendCString:\"bcdefghijklmnop\"];
	[_ms appendCString:\"qrstuvwxyz\"];
}}

- (void)buildAndSetString {{
	_ms = [[OZMutableString alloc] initWithCString:\"old content\"];
	OZString *replacement = @\"new\";
	[_ms setString:replacement];
}}

- (void)buildAndSetStringNil {{
	_ms = [[OZMutableString alloc] initWithCString:\"content\"];
	[_ms setString:nil];
}}

- (const char *)result {{
	return [_ms cString];
}}

- (unsigned int)resultLength {{
	return [_ms length];
}}

- (BOOL)hasPrefixTrue {{
	OZString *s = @\"hello world\";
	OZString *prefix = @\"hello\";
	return [s hasPrefix:prefix];
}}

- (BOOL)hasSuffixTrue {{
	OZString *s = @\"hello world\";
	OZString *suffix = @\"world\";
	return [s hasSuffix:suffix];
}}

- (BOOL)isEqualToStringTrue {{
	OZString *a = @\"hello\";
	OZString *b = @\"hello\";
	return [a isEqualToString:b];
}}

@end

#include <stdio.h>
int main(void) {{
	MutableStringTest *t = [MutableStringTest alloc];
	[t buildFromCString];
	printf(\"from_cstring=%s len=%u\\n\", [t result], [t resultLength]);
	[t buildFromOZString];
	printf(\"from_ozstring=%s\\n\", [t result]);
	[t buildWithCapacity];
	printf(\"with_capacity=%s\\n\", [t result]);
	[t buildAndAppendCString];
	printf(\"append_cstring=%s\\n\", [t result]);
	[t buildAndAppendString];
	printf(\"append_string=%s\\n\", [t result]);
	[t buildAndAppendGrow];
	printf(\"append_grow=%s\\n\", [t result]);
	[t buildAndSetString];
	printf(\"set_string=%s\\n\", [t result]);
	[t buildAndSetStringNil];
	printf(\"set_string_nil=%s\\n\", [t result]);
	printf(\"has_prefix=%d\\n\", [t hasPrefixTrue]);
	printf(\"has_suffix=%d\\n\", [t hasSuffixTrue]);
	printf(\"is_equal=%d\\n\", [t isEqualToStringTrue]);
	return 0;
}}
",
        PREAMBLE(),
        ozstring_src(),
        ozmutablestring_src()
    );
    let stdout = compile_and_run(&src, "mutable_string_basic_all_operations");
    assert_eq!(
        stdout,
        "from_cstring=hello len=5\n\
         from_ozstring=world\n\
         with_capacity=reserved\n\
         append_cstring=hello, world\n\
         append_string=hello, world\n\
         append_grow=abcdefghijklmnopqrstuvwxyz\n\
         set_string=new\n\
         set_string_nil=\n\
         has_prefix=1\n\
         has_suffix=1\n\
         is_equal=1\n"
    );
}
