#import <Foundation/OZObject.h>

@interface Color : OZObject {
        int _red;
        int _green;
        int _blue;
}
- (instancetype)initWithRed:(int)r green:(int)g blue:(int)b;
- (int)red;
@end

@implementation Color

- (instancetype)initWithRed:(int)r green:(int)g blue:(int)b {
        [super init];
        _red = r;
        _green = g;
        _blue = b;
        return self;
}

- (int)red {
        return _red;
}

@end
