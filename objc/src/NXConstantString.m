#include <objc/objc.h>

#include <zephyr/kernel.h>

@implementation NXConstantString

////////////////////////////////////////////////////////////////////////////////////
// LIFECYCLE

+ (id)alloc {
  return nil; // NXConstantString should not be allocated directly
}

- (void)dealloc
{
        /* NXConstantString is a compile-time constant and must never be freed. */
        k_oops();

        /*
         * Unreachable: satisfies GCC's "method possibly missing a [super dealloc] call"
         * check without actually calling into Object's dealloc (which would free memory).
         */
        [super dealloc];
}

////////////////////////////////////////////////////////////////////////////////////
// PROPERTIES

- (const char *)cStr {
  return _data;
}

- (unsigned int)length {
  return _length;
}

////////////////////////////////////////////////////////////////////////////////////
// PUBLIC METHODS

- (id)retain {
  // NXConstantString is immutable, so we return self
  return self;
}

- (void)release {
  // NXConstantString is immutable, so we do nothing
}

- (BOOL)isEqual:(id)anObject {
  if (self == anObject) {
    return YES;
  }
  if ([anObject class] == [self class]) {
    if (self->_length != ((NXConstantString *)anObject)->_length) {
      return NO;
    }
    return (memcmp(self->_data, ((NXConstantString *)anObject)->_data,
                   self->_length) == 0);
  }
  return NO;
}

@end
