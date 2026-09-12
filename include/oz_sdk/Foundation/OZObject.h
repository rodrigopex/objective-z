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
 * @brief Read the reference count of @p obj; 0 for nil.
 *
 * **The only refcount entry point Objective-C source may spell.** ARC
 * forbids `[obj retainCount]` outright, and the retain/release pair is
 * ARC's to insert, so this is a plain C function rather than a method.
 *
 * That sentence was true of the design and false of the implementation
 * until #428 and #436: oz_static parses with tree-sitter rather than Clang,
 * so `[obj retain]`, `[obj release]` and `[obj retainCount]` were all
 * reachable, and one of them had an emitter accommodation built for it. All
 * five selectors ARC refuses are now hard located errors, and this
 * declaration is what the diagnostics point at.
 *
 * **The companion's C API is the escape hatch, and it is a decision (#437).**
 * `oz_static_retain` and `oz_static_release` are emitted into the generated
 * companion header, so plain C in a `.m` file can drive a refcount by hand.
 * ARC governs Objective-C and has no opinion about a C call, so the
 * rejection is a rule about the source language rather than an enforced
 * invariant. Narrowing those exports away was considered and refused:
 * there is no ARC-legal Objective-C spelling that drives one shared
 * object's refcount up and down without also serialising on a slot, so a
 * two-core refcount-contention test -- `samples/smp_shared` -- could not be
 * written at all. Reaching for them means taking ownership manually and on
 * purpose. `oz_static_retain_count` below is different in kind: it reads,
 * and takes and gives no ownership, which is why it is the one an ordinary
 * program is expected to call.
 *
 * Declared here so Clang can resolve calls to it while dumping the AST,
 * which runs before any generated header exists. The companion
 * (`companion.rs`) emits the definition, with this exact signature so the
 * two declarations are redundant rather than conflicting once this header
 * is spliced into the generated C.
 *
 * Takes `id` for the same reason: the parameter type has to be spellable
 * *here*, and the root struct the companion casts to is generated. That is
 * why this is the one of the three synthesized refcount functions that
 * does not take `struct <root> *`.
 *
 * #418 collapsed a second, separately named forwarder into this one. That
 * name carried the reserved double-underscore prefix the project documents
 * as *internal*, gave one concept two public spellings, and was the SDK's
 * last `get`-prefixed getter -- `get` marks a method writing through a
 * caller's pointer (see `-getDescription:maxLength:` below), which reading
 * a count does not do.
 */
int oz_static_retain_count(id obj);

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
 * **The `get` prefix is the buffer rule, not decoration.** Objective-C
 * reserves `get` for a method that writes through a caller-supplied
 * pointer -- `-getBytes:length:range:`, `-getCharacters:range:` -- and
 * that is exactly what this does. Methods that *return* a value take no
 * prefix, which is why `OZHeap`'s `-usedBytes` has none, and why
 * `OZString`'s `-cString` keeps its own spelling: it hands back a pointer
 * to storage that already exists, so nothing is written and `get` would
 * be a lie. That asymmetry is deliberate (#413); this was
 * `-cDescription:maxLength:` until then, which abbreviated half of one
 * selector and left the buffer contract unsaid.
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
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen;
@end

/*
 * Is the inherited `-getDescription:maxLength:` the one that names the class
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
