#import <Foundation/OZObject.h>

@interface Observer : OZObject
- (void)observeValueForKeyPath:(id)keyPath
                      ofObject:(id)object
                        change:(id)change
                       context:(void *)context;
@end

@implementation Observer

- (void)observeValueForKeyPath:(id)keyPath
                      ofObject:(id)object
                        change:(id)change
                       context:(void *)context {
}

@end
