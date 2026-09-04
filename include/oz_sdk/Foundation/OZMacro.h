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
 * objects a class owns (gap N of the retired PARITY.md); the Python backend
 * compiles the same file outright.
 *
 * OZM is the way through, and it works because **a macro is the only
 * construct whose argument Objective-C leaves unparsed**. An argument
 * whose parameter is absent from the replacement list is discarded rather
 * than expanded or parsed, so it need only lex -- and `^` is a valid
 * punctuator. That is all this file does: on the Objective-C side `OZM`
 * expands to *nothing*, so the block is never type-checked.
 *
 *     OZM(K_TIMER_DEFINE, my_timer, ^(struct k_timer *t) {
 *             printk("tick\n");
 *     }, NULL);
 *
 * In the generated C the other half takes over --
 * `#define OZM(target, ...) target(__VA_ARGS__)` in
 * `include/platform/oz_platform.h`, which every generated translation
 * unit reaches through the companion header. By then oz_static has
 * replaced the block literal with the name of the function it hoisted out
 * of it, so the line becomes an ordinary
 *
 *     K_TIMER_DEFINE(my_timer, oz_block_L12_C40_1, NULL);
 *
 * The halves are in separate files, each unconditional, because each side
 * reaches exactly one of them: Objective-C never includes the PAL, and
 * this header declares no Objective-C so it is given no generated output
 * pair at all. One name serves every target macro -- there is no
 * per-primitive wrapper to write and no second arm to keep in step -- and
 * the call site still names the macro it means.
 *
 * Two limits, both worth knowing before reaching for it.
 *
 * **A hoisted block captures nothing.** It becomes a plain C function,
 * and the static bar rejects captures, so such a callback reaches its
 * context only through the channel the API itself provides
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
 * defined and the real macro provides the definition. Where the target
 * itself has a declaration idiom, prefer it and leave the referring line
 * as plain C: `samples/zbus_service` writes `ZBUS_OBS_DECLARE(...)` and
 * then an unwrapped `ZBUS_CHAN_ADD_OBS(...)`, which Clang does check.
 *
 * There was a second backend (Python, retired -- see the
 * `python-backend-final` tag) under which an `OZM` carrying an inline block
 * did not compile: that pipeline never hoisted block literals, so the `^`
 * survived into its output. Recorded because it is the reason some samples
 * carry a plain function name where a block would read better; there is no
 * longer a backend that needs the workaround.
 */
#pragma once

/*
 * Discarded, deliberately and entirely -- no parameter appears in the
 * replacement list, which is precisely why Clang never parses the
 * arguments. Guarded so that a translation unit somehow seeing both
 * halves takes this one while it is Objective-C, rather than getting a
 * redefinition.
 */
#ifdef __OBJC__
#define OZM(...)
#endif
