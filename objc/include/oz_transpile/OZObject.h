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
- (instancetype)init;
- (void)dealloc;
- (BOOL)isEqual:(id)anObject;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end
