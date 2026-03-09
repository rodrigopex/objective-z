/*
 * Behavioral spec: methods returning multiple values work correctly.
 * Based on ObjC language spec (NOT Apple code).
 * Note: Uses ivar-based approach since user-defined struct returns
 * require struct visibility across generated files (future transpiler work).
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
