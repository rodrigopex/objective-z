#import <Foundation/OZObject.h>

@interface Animal : OZObject {
        int _legs;
}
- (void)speak;
- (int)legs;
@end

@implementation Animal

- (void)speak {
}

- (int)legs {
        return _legs;
}

@end

@interface Dog : Animal {
        int _name;
}
- (void)fetch;
@end

@implementation Dog

- (void)fetch {
}

@end
