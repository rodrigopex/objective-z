// SPDX-License-Identifier: Apache-2.0
//
// array_ivars.rs -- an array ivar keeps its extent, and an array of objects
// owns its elements (#287).
//
// Two separate defects, one filed and one found while fixing it:
//
//   - An ivar declared in an `@implementation` block lost its extent, so
//     `int _values[4];` reached the generated struct as `int _values;`
//     while every use of it kept its subscript. One declared in the
//     `@interface` was always correct, because `emit::lower_ivar_decl`
//     copies that declaration through verbatim -- so only the path that
//     *rebuilds* the field from `own_ivars` was ever wrong.
//
//   - An array of objects was released as though the array itself were one
//     object: `oz_static_release((struct OZObject *)self->_leaves)` reads a
//     refcount out of the first element's pointer value. Corruption, not a
//     leak, and it compiled silently.
//
// The element tests count `-dealloc` calls rather than using a sanitizer,
// for `arc_leak_regressions.rs`'s reason: `-fsanitize=leak` is unsupported
// on arm64-apple-darwin, and a counter asks the sharper question anyway --
// not "was the memory reachable at exit" but "did teardown run".

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// The shape as filed: an extent on an ivar declared in the
/// `@implementation` block.
#[test]
fn an_implementation_block_array_keeps_its_extent() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Box : OZObject
- (int)sum;
@end

@implementation Box {
	int _values[4];
}
- (int)sum {
	_values[0] = 7;
	_values[3] = 9;
	return _values[0] + _values[3];
}
@end

#include <stdio.h>
int main(void) {
	Box *b = [[Box alloc] init];
	printf(\"sum=%d\\n\", [b sum]);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let all = format!("{}\n{}", out.companion_h, out.source_c);
    assert!(
        all.contains("int _values[4];"),
        "the extent must reach the struct field:\n{}",
        all
    );
    assert_eq!(compile_and_run(&src, "impl_block_array_keeps_extent"), "sum=16\n");
}

/// A plain scalar array owns nothing, so no release loop may be synthesized
/// for it -- the guard that the ownership test below is actually about
/// ownership.
#[test]
fn a_scalar_array_ivar_is_not_released() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Box : OZObject
- (int)first;
@end
@implementation Box {
	int _values[4];
}
- (int)first { return _values[0]; }
@end
int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("Box_oz_release_ivars"),
        "a scalar array is not owned, so nothing should be released:\n{}",
        out.source_c
    );
}

/// Every element is released when the owner is torn down -- the loop that
/// replaces the single corrupting release.
#[test]
fn an_owned_array_releases_every_element() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Leaf : OZObject
@end
@implementation Leaf
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject {
	Leaf *_leaves[2];
}
- (void)fill;
@end
@implementation Holder
- (void)fill {
	_leaves[0] = [[Leaf alloc] init];
	_leaves[1] = [[Leaf alloc] init];
}
@end

@interface Runner : OZObject
- (void)run;
@end
@implementation Runner
- (void)run {
	Holder *h = [[Holder alloc] init];
	[h fill];
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [[Runner alloc] init];
	[r run];
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run(&src, "owned_array_releases_every_element"),
        "deallocs=2\n",
        "both elements must be torn down with their owner"
    );
}

/// Overwriting a slot releases what it held. Without this the previous
/// element is unreachable by `-dealloc` time and leaks, which is the
/// difference between "the array owns its elements" and "the array owns
/// whatever happened to be stored last".
#[test]
fn overwriting_an_element_releases_what_it_held() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Leaf : OZObject
@end
@implementation Leaf
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject {
	Leaf *_leaves[1];
}
- (void)fill;
- (int)deallocsSoFar;
@end
@implementation Holder
- (void)fill {
	_leaves[0] = [[Leaf alloc] init];
	_leaves[0] = [[Leaf alloc] init];
}
- (int)deallocsSoFar { return g_deallocs; }
@end

#include <stdio.h>
int main(void) {
	Holder *h = [[Holder alloc] init];
	[h fill];
	printf(\"mid=%d\\n\", [h deallocsSoFar]);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run(&src, "overwriting_an_element_releases_it"),
        "mid=1\n",
        "the overwritten element must be released at the store, not leaked"
    );
}

/// Clearing a slot with nil releases it.
#[test]
fn clearing_an_element_with_nil_releases_it() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Leaf : OZObject
@end
@implementation Leaf
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject {
	Leaf *_leaves[1];
}
- (void)fillThenClear;
- (int)deallocsSoFar;
@end
@implementation Holder
- (void)fillThenClear {
	_leaves[0] = [[Leaf alloc] init];
	_leaves[0] = nil;
}
- (int)deallocsSoFar { return g_deallocs; }
@end

#include <stdio.h>
int main(void) {
	Holder *h = [[Holder alloc] init];
	[h fillThenClear];
	printf(\"mid=%d\\n\", [h deallocsSoFar]);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run(&src, "clearing_an_element_with_nil_releases_it"),
        "mid=1\n",
        "a nil store must release the slot's previous element"
    );
}

/// Storing a borrowed value into a slot retains it, so the owner's own
/// teardown of that value does not leave the slot dangling -- and the
/// element is released once, not twice.
#[test]
fn aliasing_into_an_element_retains_it() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Leaf : OZObject
@end
@implementation Leaf
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject {
	Leaf *_leaves[1];
}
- (void)take:(Leaf *)leaf;
@end
@implementation Holder
- (void)take:(Leaf *)leaf {
	_leaves[0] = leaf;
}
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Holder *h = [[Holder alloc] init];
	Leaf *l = [[Leaf alloc] init];
	[h take:l];
	/* `l` goes out of scope here; the slot's retain is what must keep the
	 * element alive until `h` is torn down. */
	return g_deallocs;
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [[Runner alloc] init];
	int during = [r run];
	printf(\"during=%d after=%d\\n\", during, g_deallocs);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run(&src, "aliasing_into_an_element_retains_it"),
        "during=0 after=1\n",
        "the slot's retain must outlive the local, and release exactly once"
    );
}

/// The store names its target twice, so the index is evaluated twice. An
/// index that is not provably the same both times is a located error rather
/// than a silently wrong release -- the same rule, and the same reason, as
/// the compound-assignment restriction on a dot-syntax receiver.
#[test]
fn a_side_effecting_index_is_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Leaf : OZObject
@end
@implementation Leaf
@end

@interface Holder : OZObject {
	Leaf *_leaves[2];
}
- (void)fill;
@end
@implementation Holder
- (void)fill {
	int i = 0;
	_leaves[i++] = [[Leaf alloc] init];
}
@end
int main(void) { return 0; }
"
    );
    let diags = match oz_static::transpile(&src) {
        Err(diags) => diags,
        Ok(_) => panic!("a side-effecting index must be rejected"),
    };
    let text = diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("evaluated twice"),
        "the diagnostic should say why the index is restricted, got:\n{}",
        text
    );
}

/// Dimensionality is not a limit on the extent itself: a scalar array owns
/// nothing, so any number of dimensions transpiles and indexes as the C it
/// already is -- including through the `@implementation`-block path that
/// #287 was about.
#[test]
fn a_four_dimensional_scalar_array_keeps_every_extent() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Nums : OZObject
- (int)pick;
@end
@implementation Nums {
	int _v[2][3][4][5];
}
- (int)pick {
	_v[1][2][3][4] = 11;
	return _v[1][2][3][4];
}
@end

#include <stdio.h>
int main(void) {
	Nums *n = [[Nums alloc] init];
	printf(\"pick=%d\\n\", [n pick]);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let all = format!("{}\n{}", out.companion_h, out.source_c);
    assert!(
        all.contains("int _v[2][3][4][5];"),
        "every extent must survive, not just the first:\n{}",
        all
    );
    assert_eq!(compile_and_run(&src, "four_dimensional_scalar_array"), "pick=11\n");
}

/// An owned array of objects with more than one dimension is a located
/// error. The release walks elements, and at two or more dimensions
/// `a[i]` is a sub-array -- releasing it would cast array storage to an
/// object pointer and read a refcount out of it, which is the corruption
/// the one-dimensional loop exists to remove.
///
/// Flattening with a cast to `Element **` would work everywhere and is
/// still refused: reaching across a multi-dimensional array through a
/// pointer to its first element is not defined by ISO C, and emitted C is
/// held to that.
#[test]
fn a_multi_dimensional_owned_array_is_rejected() {
    for extent in ["[2][3]", "[2][3][4][5]"] {
        let src = format!(
            "{}{}{}{}",
            PREAMBLE(),
            "\
@interface Leaf : OZObject
@end
@implementation Leaf
@end

@interface Grid : OZObject {
	Leaf *_cells",
            extent,
            ";
}
- (int)n;
@end
@implementation Grid
- (int)n { return 0; }
@end
int main(void) { return 0; }
"
        );
        let diags = match oz_static::transpile(&src) {
            Err(diags) => diags,
            Ok(_) => panic!("an owned {} array of objects must be rejected", extent),
        };
        let text = diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n");
        assert!(
            text.contains("more than one dimension") && text.contains(extent),
            "the diagnostic should name the extent, got:\n{}",
            text
        );
    }
}
