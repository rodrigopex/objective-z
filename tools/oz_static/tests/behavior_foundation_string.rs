// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_string.rs - OZ-092 Foundation work: OZString, ported
// from tests/behavior/cases/foundation/string_basic.m and
// tests/behavior/cases/inline/string_fast_access.m.
//
// Uses the real `OZObject` (`common::ozobject_src`) as the root class.
// OZString itself is transplanted from the real `src/OZString.m` (see
// `common::ozstring_src`). The interesting part isn't the class -- it's
// how `@"..."` boxes: unlike OZQ31 (a class-method call), the real
// pipeline (`tools/oz_transpile/emit.py`'s `ObjCStringLiteral` handling)
// desugars a boxed string literal directly to a static, immortal `struct
// OZString` instance -- no `alloc`/`init` at all, since every ivar
// (`_length`, `_hash`, `_data`) is compile-time-computable and `dealloc`
// is a no-op. `emit::render_boxed_string_literal` replicates that design
// (a real bug fix in this same PR -- it previously desugared to a
// nonexistent `[OZString stringWithCString:]` class-method call instead).

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE, ozstring_src};

#[test]
fn string_basic_cstring_length_and_equality() {
    // string_basic.m: getHello (cString), helloLength (length),
    // sameStringEqual (isEqual: across two separately-boxed literals with
    // identical content -- oz_static doesn't dedup like the Python oracle
    // does, so this also proves isEqual: compares content, not identity).
    let src = format!(
        "{}{}\n\
@interface StringTest : OZObject
- (const char *)getHello;
- (unsigned int)helloLength;
- (int)sameStringEqual;
@end

@implementation StringTest
- (const char *)getHello {{
	OZString *s = @\"hello\";
	return [s cString];
}}
- (unsigned int)helloLength {{
	OZString *s = @\"hello\";
	return [s length];
}}
- (int)sameStringEqual {{
	OZString *a = @\"hello\";
	OZString *b = @\"hello\";
	return [a isEqual:b];
}}
@end

#include <stdio.h>
int main(void) {{
	StringTest *t = [StringTest alloc];
	printf(\"cstr=%s\\n\", [t getHello]);
	printf(\"len=%u\\n\", [t helloLength]);
	printf(\"eq=%d\\n\", [t sameStringEqual]);
	return 0;
}}
",
        PREAMBLE(), ozstring_src()
    );
    let stdout = compile_and_run(&src, "string_basic_cstring_length_and_equality");
    assert_eq!(stdout, "cstr=hello\nlen=5\neq=1\n");
}

#[test]
fn string_fast_access_length_and_cstring_via_ivars() {
    // string_fast_access.m: stores length/cString-validity into ivars
    // during -run, read back through separate accessors -- exercises the
    // boxed literal's static struct surviving past the method that
    // created it (the OZString instance has no dynamic lifetime at all,
    // so there's nothing to outlive, but the *values* extracted from it
    // must still be correct after the fact).
    let src = format!(
        "{}{}\n\
@interface StringAccessTest : OZObject {{
	unsigned int _len;
	int _cStringValid;
}}
- (void)run;
- (unsigned int)len;
- (int)cStringValid;
@end

@implementation StringAccessTest
- (void)run {{
	OZString *s = @\"hello\";
	_len = [s length];
	const char *cs = [s cString];
	_cStringValid = (cs != 0);
}}
- (unsigned int)len {{
	return _len;
}}
- (int)cStringValid {{
	return _cStringValid;
}}
@end

#include <stdio.h>
int main(void) {{
	StringAccessTest *t = [StringAccessTest alloc];
	[t run];
	printf(\"len=%u\\n\", [t len]);
	printf(\"valid=%d\\n\", [t cStringValid]);
	return 0;
}}
",
        PREAMBLE(), ozstring_src()
    );
    let stdout = compile_and_run(&src, "string_fast_access_length_and_cstring_via_ivars");
    assert_eq!(stdout, "len=5\nvalid=1\n");
}
