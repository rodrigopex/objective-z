// SPDX-License-Identifier: Apache-2.0
//
// method_declaration_refusals.rs -- two defects a method *declaration* can
// carry that nothing looked for: a variadic ellipsis (#538) and a parameter
// name used twice (#549).
//
// Both were silent in oz2c and surfaced from **GCC, on generated C**. #549
// reported `redefinition of parameter 'amount'` against
// `oz2c_generated/Foundation/oz2c_dispatch.h` -- a file the author never
// opened, at a line that does not exist in their source. #538 was worse than
// a missing diagnostic: the ellipsis was **dropped**, so the declaration was
// altered rather than refused. A body that never reaches for `va_start`
// compiles silently as a fixed-arg function, and the one that does failed on
// `'va_start' used in function with fixed arguments` -- a cause that is a
// symptom of the drop.
//
// The ellipsis parses as the anonymous token `"..."`, **not** as
// `variadic_parameter`. That type exists in tree-sitter-objc's
// `node-types.json` and belongs to a plain C parameter list; matching it here
// found nothing. Settled by dumping the tree for the fragment, which is the
// same way #367's `typedefed_specifier` was settled after `generic_specifier`
// looked obvious and was wrong.

mod common;
use common::{expect_reject, ozobject_src as PREAMBLE};

/// The variadic method (#538), refused with the caret under the ellipsis.
#[test]
fn a_variadic_method_is_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Summer : OZObject
- (int)sumOf:(int)count, ...;
@end
@implementation Summer
- (int)sumOf:(int)count, ... { return count; }
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("a variadic Objective-C method is not supported"),
        "diagnostics:\n{}",
        diags
    );
    /* The remedy an author will otherwise go looking for: `OZLog` is
     * variadic and works, so the message has to say why a *method* cannot
     * be. */
    assert!(diags.contains("OZLog"), "diagnostics:\n{}", diags);
}

/// The duplicate parameter name (#549).
///
/// Named in the message, because the author has to know *which* one --
/// a method with four parameters gives the message its whole value.
#[test]
fn a_duplicate_parameter_name_is_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Adder : OZObject
- (int)addA:(int)amount andB:(int)amount;
@end
@implementation Adder
- (int)addA:(int)amount andB:(int)amount { return amount; }
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("parameter 'amount' is declared more than once"),
        "diagnostics:\n{}",
        diags
    );
    /* A `!contains("oz2c_dispatch.h")` negative cannot work here, and the
     * reason is worth recording: the diagnostic's own *note* names that
     * file, explaining where the error used to surface. The needle would
     * match the prose written about the defect rather than the defect --
     * the same trap as a guard whose subject is text you also wrote.
     *
     * The property actually wanted is that the diagnostic is located at
     * the author's declaration, which is asserted positively: the message
     * names the parameter, and the note explains the history a reader
     * coming from the old GCC error needs. */
    assert!(
        diags.contains("oz2c_dispatch.h"),
        "the note must say where this used to surface, or a reader who saw the old \
         GCC error cannot connect the two:\n{}",
        diags
    );
    assert!(
        diags.contains("rename this 'amount'"),
        "the remedy names the parameter, which is the message's whole value in a \
         method with four of them:\n{}",
        diags
    );
}

/// The accepting controls, and the reason they are here: a check that
/// matched a bare `"..."` token anywhere, or compared parameter names
/// across *different* methods, would pass both tests above and refuse
/// ordinary code.
#[test]
fn ordinary_declarations_are_still_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Fine : OZObject
/* Two parameters, distinct names -- the shape #549 must not touch. */
- (int)addA:(int)first andB:(int)second;
/* The *same* name as a parameter of another method, which is legal: the
   duplicate rule is per-method, not per-class. */
- (int)scale:(int)first;
@end
@implementation Fine
- (int)addA:(int)first andB:(int)second { return first + second; }
- (int)scale:(int)first { return first * 2; }
@end

#include <stdio.h>
int main(void)
{
	Fine *f = [[Fine alloc] init];

	printf(\"sum=%d scaled=%d\\n\", [f addA:2 andB:3], [f scale:4]);
	return 0;
}
"
    );
    let stdout = common::compile_and_run(&src, "method_decl_controls");
    assert_eq!(stdout, "sum=5 scaled=8\n");
}

/// A *plain C* variadic function is untouched -- `OZLog` is one, and
/// refusing it would break the SDK.
///
/// This is the control for the `"..."` token match: the token appears in a
/// C `parameter_list` too, and a check that walked for it without requiring
/// a `method_declaration` parent would refuse this.
#[test]
fn a_plain_c_variadic_function_is_untouched() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
#include <stdarg.h>
#include <stdio.h>

static int sum_all(int count, ...)
{
	va_list ap;
	int total = 0;

	va_start(ap, count);
	for (int i = 0; i < count; i++) {
		total += va_arg(ap, int);
	}
	va_end(ap);
	return total;
}

int main(void)
{
	printf(\"total=%d\\n\", sum_all(3, 1, 2, 3));
	return 0;
}
"
    );
    let stdout = common::compile_and_run(&src, "c_variadic_untouched");
    assert_eq!(stdout, "total=6\n", "a plain C variadic function is not a method");
}
