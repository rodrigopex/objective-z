#import <Foundation/OZAutoreleasePool.h>
#import <objc/objc.h>
#import <zephyr/logging/log.h>
LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

/* refcount.c callback registration */
extern void __objc_refcount_set_autorelease_fn(void (*fn)(id));

/* Per-thread pool stack (requires CONFIG_THREAD_LOCAL_STORAGE) */
static __thread OZAutoreleasePool *_currentPool = nil;

/**
 * @brief C callback for refcount.c's autorelease support.
 */
static void __objc_arp_add_object(id obj)
{
	[OZAutoreleasePool addObject:obj];
}

@implementation OZAutoreleasePool

+ (void)initialize
{
	/* Register our callback so refcount.c can autorelease without
	 * a compile-time dependency on OZAutoreleasePool */
	__objc_refcount_set_autorelease_fn(__objc_arp_add_object);
}

+ (void)addObject:(id)obj
{
	if (_currentPool == nil) {
		LOG_ERR("autorelease with no pool in place -- leaking %p", (void *)obj);
		return;
	}
	if (_currentPool->_count >= OBJZ_ARP_CAPACITY) {
		LOG_ERR("autorelease pool overflow (%u objects)", OBJZ_ARP_CAPACITY);
		k_panic();
		return;
	}
	_currentPool->_objects[_currentPool->_count++] = obj;
}

- (id)init
{
	self = [super init];
	if (self) {
		_count = 0;
		_parent = _currentPool;
		_currentPool = self;
		LOG_DBG("pool push %p (parent=%p)", self, _parent);
	}
	return self;
}

- (void)drain
{
	LOG_DBG("pool drain %p (%u objects)", self, _count);

	/* Release objects in reverse order */
	while (_count > 0) {
		_count--;
		id obj = _objects[_count];
		_objects[_count] = nil;
		if (obj != nil) {
			[obj release];
		}
	}

	/* Restore parent pool */
	_currentPool = _parent;
	_parent = nil;

	/* Free pool itself */
	[super dealloc];
}

@end

/* C helpers for ARC entry points and @autoreleasepool {} syntax */

void *__objc_autoreleasepool_push(void)
{
	return [[OZAutoreleasePool alloc] init];
}

void __objc_autoreleasepool_pop(void *token)
{
	[(OZAutoreleasePool *)token drain];
}

/*
 * gnustep-1.7 @autoreleasepool {} emits calls to these non-prefixed
 * symbols. Bridge to the __objc_ versions above.
 */
void *objc_autoreleasePoolPush(void)
{
	return __objc_autoreleasepool_push();
}

void objc_autoreleasePoolPop(void *token)
{
	__objc_autoreleasepool_pop(token);
}
