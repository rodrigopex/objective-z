/**
 * @file OZObject.h
 * @brief Root class for OZ transpiler samples.
 *
 * Lightweight ObjC interface that Clang can parse without Zephyr
 * generated headers.  The transpiler emits a pure-C struct and
 * retain/release/alloc/free helpers from this declaration.
 */

 #pragma once
 #include <stdbool.h>
 #include <stddef.h>
 #include <stdint.h>

 /** @brief A null object pointer.
  * @ingroup objc
  */
 #define nil ((id)0)

 /* There is deliberately no `Nil` here. The transpiler emits one into the
  * generated C -- 0xFFFF, which a 10-bit `class_id` can never hold -- but
  * it cannot be spelled in Objective-C source: `Class` is a pointer to
  * Clang, which rejects the cast under ARC ("cast of 'int' to 'Class' is
  * disallowed with ARC"), and defining it as `((Class)0)` for Clang's
  * benefit would make the same comparison mean two different things in
  * the AST dump and in the emitted C. The nil contract is observable
  * without it: -isMemberOfClass:, -isKindOfClass: and
  * -respondsToSelector: answer NO for a nil receiver, and
  * -performSelector: answers nil -- including against the root class,
  * which only holds because a nil receiver's class matches nothing.
  */

 // Booleans

 /** @brief A Boolean value.
  * @ingroup objc
  */
 typedef bool BOOL;

 /** @brief The Boolean value `true`.
  * @ingroup objc
  */
 #define YES true

 /** @brief The Boolean value `false`.
  * @ingroup objc
  */
 #define NO false

/**
 * @brief Read the reference count of an object.
 *
 * Declared here so Clang can resolve calls during AST dump.
 * The transpiler emits a macro in the generated OZObject.h.
 */
unsigned int __objc_refcount_get(id obj);

__attribute__((objc_root_class))
@interface OZObject
{
	int _refcount;
}
+ (instancetype)alloc;
+ (instancetype)allocWithHeap:(id)heap;
+ (Class)class;
- (Class)class;
- (BOOL)isMemberOfClass:(Class)aClass;
- (BOOL)isKindOfClass:(Class)aClass;
- (BOOL)conformsToProtocol:(Protocol *)aProtocol;
- (BOOL)respondsToSelector:(SEL)aSelector;
- (id)performSelector:(SEL)aSelector;
- (id)performSelector:(SEL)aSelector withObject:(id)object;
- (id)performSelector:(SEL)aSelector withObject:(id)object withObject:(id)otherObject;
- (instancetype)init;
- (void)dealloc;
- (BOOL)isEqual:(id)anObject;
- (int)cDescription:(char *)buf maxLength:(size_t)maxLen;
@end

#ifdef __clang__
@compatibility_alias NSObject OZObject;
#endif
