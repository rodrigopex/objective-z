#include <objc/objc.h>

#include <zephyr/kernel.h>

@implementation OZString

/* OZString should not be allocated directly */
+ (id)alloc {
  return nil;
}

- (void)dealloc
{
        /* OZString is a compile-time constant and must never be freed. */
        k_oops();

        /* Unreachable: satisfies GCC's "method possibly missing a
         * [super dealloc] call" check. */
        [super dealloc];
}

- (const char *)cStr {
  return _data;
}

- (unsigned int)length {
  return _length;
}

- (id)description {
  return (id)self;
}

- (id)retain {
  return self;
}

- (oneway void)release {
  /* no-op for immutable constant strings */
}

- (BOOL)isEqual:(id)anObject {
  if (self == anObject) {
    return YES;
  }
  if ([anObject class] == [self class]) {
    OZString *other = (OZString *)anObject;
    /* Fast rejection: compare cached hash if both are non-zero */
    if (self->_hash != 0 && other->_hash != 0 && self->_hash != other->_hash) {
      return NO;
    }
    if (self->_length != other->_length) {
      return NO;
    }
    return (memcmp(self->_data, other->_data, self->_length) == 0);
  }
  return NO;
}

@end
