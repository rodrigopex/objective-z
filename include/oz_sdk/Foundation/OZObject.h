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

 /* `OZObjectProtocol`, the protocol this class adopts and that every other
  * protocol should adopt in turn (#307). Imported *here*, after `BOOL` and
  * `YES`: that header declares methods returning `BOOL` and `size_t` and
  * deliberately defines neither, since this file importing it makes the
  * reverse edge a cycle. It says so with an `#error` if reached first.
  */
 #import "OZObjectProtocol.h"

/**
 * @brief Read the reference count of an object.
 *
 * Declared here so Clang can resolve calls during AST dump.
 * The transpiler emits a macro in the generated OZObject.h.
 */
unsigned int __objc_refcount_get(id obj);

__attribute__((objc_root_class))
@interface OZObject <OZObjectProtocol>
+ (instancetype)alloc;
/**
 * @brief Allocate from the system heap (`k_malloc` on Zephyr).
 *
 * The dynamic counterpart to `+alloc`, which takes a slot from the
 * class's static slab. Static versus dynamic is the axis these two
 * names divide on, and the system heap is the ordinary case on Zephyr
 * -- which is why it gets the short name and a named heap is the one
 * that takes an argument.
 *
 * Needs `CONFIG_OBJZ_HEAP=y`; without it this is a located transpile
 * error rather than a silent fallback to the slab.
 */
+ (instancetype)dynamicAlloc;
/** @brief Allocate from @p heap, or from the system heap when it is nil. */
+ (instancetype)dynamicAllocWithHeap:(id)heap;
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
/**
 * @brief Writes a C description of the receiver into @p buf.
 *
 * The hook `OZLog`'s `%@` dispatches to. Returns the number of
 * characters written, never more than @p maxLen.
 *
 * `OZObject`'s own implementation is the **default** every class
 * inherits until it overrides this, and it writes
 * `<ClassName: 0xADDRESS>` -- the shape Objective-C's `-description`
 * defaults to. It used to write nothing at all, so `%@` on any class
 * without its own description produced an empty field, which is
 * indistinguishable from a description that really is empty and from a
 * formatting bug (#354).
 *
 * `CONFIG_OBJZ_DEFAULT_DESCRIPTION=n` restores the old no-op and gets
 * back the ~360 bytes it costs -- which it costs on every program that
 * links `OZLog`, not only those using `%@`. See the option's help text
 * and `docs/STATUS.md` for why the linker cannot drop it.
 */
- (int)cDescription:(char *)buf maxLength:(size_t)maxLen;
@end

/*
 * Is the inherited `-cDescription:maxLength:` the one that names the class
 * (#354), or the no-op it used to be?
 *
 * Keyed on `CONFIG_OBJZ_DEFAULT_DESCRIPTION`, which only a Zephyr build
 * defines -- and read through `CONFIG_OBJZ` rather than `__ZEPHYR__` so
 * that a host build of the test corpora, which defines neither, always
 * gets the default. Without that asymmetry the host suites would silently
 * exercise the disabled path and the option's own tests could not tell the
 * two apart.
 *
 * Overridable from the command line, which is how
 * `tests/default_description.rs` reaches the disabled path on a host
 * without a Kconfig at all.
 */
#ifndef OZ_DEFAULT_DESCRIPTION
#  if !defined(CONFIG_OBJZ) || defined(CONFIG_OBJZ_DEFAULT_DESCRIPTION)
#    define OZ_DEFAULT_DESCRIPTION 1
#  else
#    define OZ_DEFAULT_DESCRIPTION 0
#  endif
#endif

/**
 * @brief The receiver's class name, as a string literal.
 *
 * Synthesized by the transpiler into the companion source, where the
 * class-id switch it needs is available (`companion.rs`). Declared here
 * rather than only in the companion header because `src/OZObject.m` calls
 * it, and that file is also compiled on its own for the Clang AST dump --
 * which never sees a generated header.
 *
 * Answers `"nil"` for a nil receiver and `"?"` for a class id this program
 * does not know, so the caller never has to check.
 */
const char *oz_static_class_name(OZObject *self);

#ifdef __clang__
@compatibility_alias NSObject OZObject;
#endif
