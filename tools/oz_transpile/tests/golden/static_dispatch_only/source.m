#import <Foundation/OZObject.h>

@interface Timer : OZObject {
        int _interval;
}
- (void)start;
- (void)stop;
@end

@implementation Timer

- (void)start {
}

- (void)stop {
}

@end

@interface Logger : OZObject {
        int _level;
}
- (void)log;
@end

@implementation Logger

- (void)log {
}

@end
