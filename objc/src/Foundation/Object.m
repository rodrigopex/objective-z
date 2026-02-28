#import <Foundation/OZMutableString.h>
#import <objc/objc.h>
#include <zephyr/sys/printk.h>
#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

#ifdef CONFIG_OBJZ_STATIC_POOLS
extern void *__objc_pool_alloc(Class cls);
extern _Bool __objc_pool_free(id obj);
#endif

/* refcount.c functions */
extern id __objc_refcount_retain(id obj);
extern bool __objc_refcount_release(id obj);
extern unsigned int __objc_refcount_get(id obj);
extern void __objc_refcount_set(id obj, long value);
extern id __objc_autorelease_add(id obj);

@implementation Object

+ (void)initialize
{
	/* No-op */
}

+ (id)alloc
{
	LOG_DBG("allocating instance of class %s", class_getName(self));
	size_t size = class_getInstanceSize(self);
	id obj = nil;

#ifdef CONFIG_OBJZ_STATIC_POOLS
	obj = (id)__objc_pool_alloc(self);
#endif
	if (obj == nil) {
		obj = (id)objc_malloc(size);
		if (obj) {
			memset(obj, 0, size);
		}
	}
	if (obj) {
		object_setClass(obj, self);
		__objc_refcount_set(obj, 1);
	}
	return obj;
}

- (id)init
{
	return self;
}

- (void)dealloc
{
	LOG_DBG("deallocating instance of class %s", class_getName(object_getClass(self)));

#ifdef CONFIG_OBJZ_STATIC_POOLS
	if (__objc_pool_free(self)) {
		return;
	}
#endif
	objc_free(self);
}

- (id)retain
{
	__objc_refcount_retain(self);
	LOG_DBG("Object -retain %s rc=%u", class_getName([self class]),
		__objc_refcount_get(self));
	return self;
}

- (oneway void)release
{
	LOG_DBG("Object -release %s rc=%u", class_getName([self class]),
		__objc_refcount_get(self));
	if (__objc_refcount_release(self)) {
		[self dealloc];
	}
}

- (id)autorelease
{
	LOG_DBG("Object -autorelease %s rc=%u", class_getName([self class]),
		__objc_refcount_get(self));
	return __objc_autorelease_add(self);
}

- (unsigned int)retainCount
{
	return __objc_refcount_get(self);
}

- (Class)class
{
	return object_getClass(self);
}

+ (Class)class
{
	return self;
}

- (Class)superclass
{
	return object_getSuperclass(self);
}

+ (Class)superclass
{
	return class_getSuperclass(self);
}

+ (const char *)name
{
	return class_getName(self);
}

- (BOOL)isEqual:(id)anObject
{
	return self == anObject;
}

- (BOOL)isKindOfClass:(Class)cls
{
	return object_isKindOfClass(self, cls);
}

/**
 * @brief Checks if the class conforms to a protocol.
 */
+ (BOOL)conformsTo:(Protocol *)aProtocolObject
{
	return class_conformsTo(self, (objc_protocol_t *)aProtocolObject);
}

/**
 * @brief Checks if the receiver's class conforms to a protocol.
 */
- (BOOL)conformsTo:(Protocol *)aProtocolObject
{
	return class_conformsTo(object_getClass(self), (objc_protocol_t *)aProtocolObject);
}

/**
 * @brief Checks if the receiver responds to a selector.
 */
- (BOOL)respondsToSelector:(SEL)aSelector
{
	return object_respondsToSelector(self, aSelector);
}

- (id)description
{
	char buf[64];
	snprintk(buf, sizeof(buf), "<%s: %p>", class_getName(object_getClass(self)), self);
	return [OZMutableString stringWithCString:buf];
}

- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	return snprintk(buf, maxLen, "<%s: %p>", class_getName(object_getClass(self)), self);
}

@end
