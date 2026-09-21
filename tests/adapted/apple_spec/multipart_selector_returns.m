/*
 * Proves: a two-part selector delivers both arguments to the right ivars
 * (`-setOriginX:y:`, `-setWidth:height:`), and a method whose return value is
 * computed rather than stored answers the computation (`-area`, `-perimeter`).
 *
 * Does not prove: anything about struct-by-value returns. This file was named
 * `struct_return` until #596 and declared no struct; the note it carried --
 * "user-defined struct returns require struct visibility across generated
 * files (future transpiler work)" -- was itself stale. Struct return works and
 * is covered by `tools/oz2c/tests/nil_receiver.rs` (a bare `struct`, a
 * typedef'd one, and a typedef'd union) and by `split_output.rs`, which links
 * three struct-returning methods across a header/impl split. A bare `union`
 * return is the one broken shape (#595).
 *
 * Inspiration: the Objective-C language specification's rule for keyword
 * selectors. No Apple objc4 test corresponds -- this case has no Apple
 * provenance and sits in `apple_spec/` for historical reasons only; see this
 * directory's README.
 * Upstream revision: not pinned at authoring time (pre-#596).
 * Omitted from upstream: nothing; there is no upstream test behind it.
 * Authorship: independently written. No APSL-licensed code copied.
 */
#import "OZTestBase.h"

@interface Geometry : OZObject {
	int _originX;
	int _originY;
	int _width;
	int _height;
}
- (int)originX;
- (int)originY;
- (int)width;
- (int)height;
- (void)setOriginX:(int)x y:(int)y;
- (void)setWidth:(int)w height:(int)h;
- (int)area;
- (int)perimeter;
@end

@implementation Geometry
- (int)originX { return _originX; }
- (int)originY { return _originY; }
- (int)width { return _width; }
- (int)height { return _height; }
- (void)setOriginX:(int)x y:(int)y { _originX = x; _originY = y; }
- (void)setWidth:(int)w height:(int)h { _width = w; _height = h; }
- (int)area { return _width * _height; }
- (int)perimeter { return 2 * (_width + _height); }
@end
