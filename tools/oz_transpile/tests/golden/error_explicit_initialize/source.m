#import <Foundation/OZObject.h>

@interface AppConfig : OZObject
+ (void)initialize;
- (void)doWork;
@end

@implementation AppConfig

+ (void)initialize {
}

- (void)doWork {
        [AppConfig initialize];
}

@end
