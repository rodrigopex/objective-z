/* Platform Abstraction Layer — ifdef router */
#ifndef OZ_PLATFORM_H
#define OZ_PLATFORM_H

#include "oz_platform_types.h"
#include "oz_assert.h"

#ifdef OZ_PLATFORM_ZEPHYR
#include "oz_platform_zephyr.h"
#elif defined(OZ_PLATFORM_HOST)
#include "oz_platform_host.h"
#else
#error "Define OZ_PLATFORM_ZEPHYR or OZ_PLATFORM_HOST"
#endif

/* Fallback stub for struct oz_heap_inner when heap is not enabled.
 *
 * OZ_HEAP_INNER_DEFINED is set here, as the two PAL backends already do
 * when they define the real struct under OZ_HEAP_SUPPORT. Testing the
 * guard without ever setting it left every fallback stub still enabled,
 * so two of them in one translation unit was a redefinition -- which is
 * what an ARM build of any sample hit as soon as the static backend
 * became the default: it splices oz_sdk/Foundation/OZHeap.h's identical
 * stub into its generated OZHeap.h, and that file includes this one.
 */
#ifndef OZ_HEAP_INNER_DEFINED
#define OZ_HEAP_INNER_DEFINED
struct oz_heap_inner {
        int _opaque;
};

/* The guard covers the accessors as well as the struct, so this fallback
 * has to supply both -- oz_sdk/Foundation/OZHeap.h declares them inside
 * the same #ifndef, and whichever of the two blocks compiles first now
 * suppresses the other entirely. Without these, a build with heap support
 * off reached OZHeap's own -initWithBuffer:size: (spliced in because
 * Foundation.h imports OZHeap.h, whether or not anything calls it) and
 * failed on an implicit declaration of oz_heap_init.
 *
 * No-ops rather than an error: with no heap configured there is nothing to
 * initialise and nothing in use, so 0 bytes used is the honest answer.
 */
static inline void oz_heap_init(struct oz_heap_inner *inner, void *buf,
                                size_t size)
{
        (void)inner;
        (void)buf;
        (void)size;
}

static inline size_t oz_heap_used_bytes(struct oz_heap_inner *inner)
{
        (void)inner;
        return 0;
}
#endif

/* ------------------------------------------------------------------ */
/* OZM — the C half. See include/oz_sdk/Foundation/OZMacro.h.          */
/* ------------------------------------------------------------------ */

/*
 * `OZM(MACRO, a, b)` becomes `MACRO(a, b)`, by preprocessor alone:
 * `target` is substituted and the replacement list is then rescanned,
 * which expands the target macro. No transpiler rule is involved.
 *
 * The two halves live apart because each side reaches exactly one of
 * them, which is what makes them unconditional:
 *
 *   - Objective-C sees only `OZMacro.h`, in the Foundation umbrella,
 *     where `OZM(...)` is *empty*. That is the whole trick: an argument
 *     whose parameter is absent from the replacement list is discarded
 *     rather than parsed, so Clang never type-checks a block handed to a
 *     macro that wants a function pointer -- which it would refuse.
 *   - Generated C sees only this file, which every generated translation
 *     unit reaches through the companion header. `OZMacro.h` declares no
 *     Objective-C, so it is given no output pair at all and its
 *     definition never arrives here -- which is why this half cannot
 *     live there.
 *
 * By this point oz_static has replaced any block literal in the
 * arguments with the name of the function it hoisted out of it, so the
 * target macro receives a function pointer, which is what
 * `Z_TIMER_INITIALIZER`'s `.expiry_fn` and `zbus_observer::callback`
 * need.
 *
 * Two things deliberately not done, both tested first:
 *
 *   - No `OZM_DEFER` indirection. The canonical spelling of this idiom
 *     adds a level so the replacement list is rescanned once more, and it
 *     is not needed here: the single-level form expands a plain target
 *     and an aliased one (`#define MY_ALIAS K_TIMER_DEFINE`) alike.
 *   - No support for `OZM(MACRO)` with nothing after the name. That
 *     leaves `__VA_ARGS__` empty, and passing no argument for a variadic
 *     parameter is a C23 extension -- `-std=c17 -pedantic-errors`, which
 *     the corpus is gated on, rejects it. Every definition macro this
 *     exists for takes arguments.
 */
#ifndef __OBJC__
#define OZM(target, ...) target(__VA_ARGS__)
#endif

#endif /* OZ_PLATFORM_H */
