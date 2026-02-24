#import <objc/OZObject.h>
#import <objc/objc.h>
#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

/* refcount.c functions */
extern id __objc_refcount_retain(id obj);
extern bool __objc_refcount_release(id obj);
extern unsigned int __objc_refcount_get(id obj);
extern void __objc_refcount_set(id obj, long value);
extern id __objc_autorelease_add(id obj);

@implementation OZObject

+ (id)alloc
{
	id obj = [super alloc];
	if (obj) {
		__objc_refcount_set(obj, 1);
		LOG_DBG("OZObject +alloc %s rc=1", class_getName(self));
	}
	return obj;
}

- (id)init
{
	return self;
}

- (id)retain
{
	__objc_refcount_retain(self);
	LOG_DBG("OZObject -retain %s rc=%u", class_getName([self class]),
		__objc_refcount_get(self));
	return self;
}

- (oneway void)release
{
	LOG_DBG("OZObject -release %s rc=%u", class_getName([self class]),
		__objc_refcount_get(self));
	if (__objc_refcount_release(self)) {
		[self dealloc];
	}
}

- (id)autorelease
{
	LOG_DBG("OZObject -autorelease %s rc=%u", class_getName([self class]),
		__objc_refcount_get(self));
	return __objc_autorelease_add(self);
}

- (unsigned int)retainCount
{
	return __objc_refcount_get(self);
}

- (void)dealloc
{
	LOG_DBG("OZObject -dealloc %s", class_getName([self class]));
	[super dealloc];
}

@end
