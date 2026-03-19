#import <Foundation/OZObject.h>

@protocol Drawable
- (void)draw;
- (int)color;
@end

@interface Circle : OZObject <Drawable> {
        int _radius;
}
- (void)draw;
- (int)color;
@end

@implementation Circle

- (void)draw {
}

- (int)color {
        return 1;
}

@end

@interface Square : OZObject <Drawable> {
        int _side;
}
- (void)draw;
- (int)color;
@end

@implementation Square

- (void)draw {
}

- (int)color {
        return 2;
}

@end
