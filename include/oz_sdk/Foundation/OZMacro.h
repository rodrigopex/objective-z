/**
 * @file OZMacro.h
 * @brief OZM -- write a target definition macro with an inline block.
 *
 * A target's static definition macros take a callback as a function
 * pointer:
 *
 *     K_TIMER_DEFINE(name, expiry_fn, stop_fn)
 *     ZBUS_LISTENER_DEFINE(name, callback)
 *
 * Objective-C refuses block-to-function-pointer conversion in *every*
 * position -- by cast or by initialization, with ARC or without -- so
 * handing one an inline block is rejected by Clang:
 *
 *     error: initializing 'void (*)(int)' with an expression of
 *            incompatible type 'void (^)(int)'
 *
 * and Clang is not optional here. `cmake/oz_static.cmake` dumps one Clang
 * AST per source, and that dump is the only authority on which ivars are
 * objects a class owns (PARITY.md gap N); the outgoing Python backend
 * compiles the same file outright. A source Clang rejects also fails
 * *silently* -- the dump is taken with `2>/dev/null || true` and the "no
 * usable AST" warning fires only when every dump is unusable -- so one such
 * file would quietly lose its own ARC facts and leak its `id` ivars.
 *
 * OZM is the way through, and it works because **a macro is the only
 * construct whose argument Objective-C leaves unparsed**. An argument whose
 * parameter is absent from the replacement list is discarded rather than
 * expanded or parsed, so it need only lex -- and `^` is a valid punctuator.
 * Clang therefore never type-checks the block, while oz_static rewrites
 * `OZM(MACRO, ...)` back to `MACRO(...)` for the C compiler, by which point
 * the block literal has become the name of a function hoisted out of it:
 *
 *     OZM(K_TIMER_DEFINE, my_timer, ^(struct k_timer *t) {
 *             printk("tick\n");
 *     }, NULL);
 *
 * becomes
 *
 *     void oz_block_L12_C40_1(struct k_timer *t) { printk("tick\n"); }
 *     K_TIMER_DEFINE(my_timer, oz_block_L12_C40_1, NULL);
 *
 * One name serves every target macro -- there is no per-primitive wrapper
 * to write and no second arm to keep in step -- and the call site still
 * names the macro it means.
 *
 * Two limits, both worth knowing before reaching for it.
 *
 * **A hoisted block captures nothing.** It becomes a plain C function, and
 * the static bar rejects captures, so such a callback reaches its context
 * only through the channel the API itself provides
 * (`k_timer_user_data_get`, `zbus_chan_const_msg`). That is the same
 * constraint Zephyr's own C callbacks live under.
 *
 * **What the macro declares is invisible to Clang.** Since the whole
 * invocation is discarded on that side, `OZM(K_TIMER_DEFINE, my_timer, ...)`
 * leaves no `my_timer` for Clang to see. If Objective-C code in the same
 * file refers to it -- `k_timer_start(&my_timer, ...)` -- declare it for
 * Clang alone:
 *
 *     #ifdef __OBJC__
 *     static struct k_timer my_timer;
 *     #endif
 *
 * That block is passed through to the generated C, where `__OBJC__` is not
 * defined and the real macro provides the definition. Code that only ever
 * names the object inside other `OZM(...)` invocations needs nothing, since
 * those are discarded too.
 *
 * **This is an oz_static feature.** The rewrite is `emit::ozm_edits`, so
 * under `CONFIG_OBJZ_BACKEND_PYTHON` the Objective-C arm is the only one
 * there is: the invocation expands to nothing and the definition never
 * happens, leaving a program that builds and never registers its callback.
 * oz_static is the default backend and the Python pipeline is the outgoing
 * one, so the trade is deliberate -- but it is a silent behavioural
 * difference rather than a build failure, which is worth knowing before
 * putting `OZM` in code that has to work under both.
 */
#pragma once

#ifdef __OBJC__
/*
 * Discarded, deliberately and entirely. No parameter appears in the
 * replacement list, which is precisely why Clang never parses the
 * arguments -- see this file's header. oz_static rewrites the invocation
 * into the real macro call, so this arm is never the one that runs code.
 */
#define OZM(...)
#else
/*
 * Reached only if generated C somehow kept an `OZM(...)` that oz_static
 * should have rewritten. Left as a hard error rather than a silent
 * expansion to nothing, which would drop a timer or a listener and leave a
 * program that builds and does not work. oz_static never degrades quietly.
 */
#define OZM(...) _Static_assert(0, "OZM(...) reached the C compiler unrewritten -- oz_static should have turned it into its first argument")
#endif
