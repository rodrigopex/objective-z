/**
 * @file OZObjectProtocol.h
 * @brief OZObjectProtocol -- what every OZ object answers to.
 *
 * This is oz_sdk's `<NSObject>`: the protocol a *protocol* adopts so that
 * a protocol-qualified receiver can still be introspected.
 *
 * Clang resolves a message sent to `id<P>` against `P` and its
 * super-protocols and nowhere else -- the root class is unreachable from
 * such a type. So without this,
 *
 *     id<PXToggleable> indicator = ...;
 *     [indicator conformsToProtocol:@protocol(PXDimmable)];
 *
 * is `error: no known instance method for selector 'conformsToProtocol:'`
 * even though `OZObject` declares it and the generated C dispatches it
 * correctly either way (#307). Real Objective-C has the same rule and the
 * same answer: `<NSObject>` declares the introspection methods, and
 * `@protocol Foo <NSObject>` is why `id<NSCopying>` can be asked
 * `respondsToSelector:`.
 *
 * So a protocol whose receivers might be introspected adopts it:
 *
 *     @protocol PXToggleable <OZObjectProtocol>
 *     - (void)toggle;
 *     @end
 *
 * A class conforming to such a protocol implements nothing extra: every
 * method here is declared and defined by `OZObject`, Clang counts an
 * inherited implementation as satisfying a protocol requirement, and
 * since #307 so does oz2c (`Program::implements_selector`).
 *
 * **This header is reached through `OZObject.h`, never on its own.** It
 * needs `BOOL` and `size_t`, which `OZObject.h` defines, and `OZObject.h`
 * imports this file in order to adopt the protocol -- so the definitions
 * cannot live here without a cycle, and they cannot move here either:
 * `nil` and friends are file-scope macros that the emitter makes visible
 * to every generated `.c` by way of the *root class's* own header, and
 * that mechanism is keyed on classes (`emit::always_visible` reads
 * `class_to_stem`). An origin declaring no class has no stem to hang the
 * edge on, so moving them produced
 * `OZObject.c:18: use of undeclared identifier 'nil'` in every corpus
 * case. The `#error` below turns that ordering requirement from
 * something to remember into something the compiler says.
 */
#pragma once

/* `__OBJC__`-guarded, because the ordering it enforces is an
 * Objective-C-side one. oz2c gives this origin a generated `.h`/`.c` pair
 * like any other, the `@protocol` block is elided from it (protocols are
 * a compile-time contract, never emitted), and the resulting
 * `OZObjectProtocol.c` is compiled on its own -- including only its own
 * header, where an unguarded `#error` fires every time. */
#if defined(__OBJC__) && !defined(YES)
#error "OZObjectProtocol.h is reached through OZObject.h, which defines BOOL/YES/NO; import that instead"
#endif

/**
 * @brief What every OZ object answers to.
 *
 * Every declaration here is copied from `OZObject`'s own interface and
 * must stay identical to it: `generics::check_dispatch_signature_agreement`
 * compares a protocol's declared return type against each implementor's,
 * and a disagreement is a located error.
 */
@protocol OZObjectProtocol
@required
- (Class)class;
- (BOOL)isMemberOfClass:(Class)aClass;
- (BOOL)isKindOfClass:(Class)aClass;
- (BOOL)conformsToProtocol:(Protocol *)aProtocol;
- (BOOL)respondsToSelector:(SEL)aSelector;
- (id)performSelector:(SEL)aSelector;
- (id)performSelector:(SEL)aSelector withObject:(id)object;
- (id)performSelector:(SEL)aSelector withObject:(id)object withObject:(id)otherObject;
- (BOOL)isEqual:(id)anObject;
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen;
@end
