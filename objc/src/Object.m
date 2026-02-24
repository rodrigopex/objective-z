#import <objc/objc.h>
#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

#ifdef CONFIG_OBJZ_STATIC_POOLS
extern void *__objc_pool_alloc(const char *class_name);
extern _Bool __objc_pool_free(void *ptr);
#endif

@implementation Object

+ (void)initialize
{
	// No-op
}

+ (id)alloc
{
	LOG_DBG("allocating instance of class %s", class_getName(self));
	size_t size = class_getInstanceSize(self);
	id obj = nil;

#ifdef CONFIG_OBJZ_STATIC_POOLS
	obj = (id)__objc_pool_alloc(class_getName(self));
#endif
	if (obj == nil) {
		obj = (id)objc_malloc(size);
		if (obj) {
			memset(obj, 0, size);
		}
	}
	if (obj) {
		object_setClass(obj, self);
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

@end
