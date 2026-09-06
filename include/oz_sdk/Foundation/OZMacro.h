/**
 * @file OZMacro.h
 * @brief OZM and OZFN -- write an inline block where a function pointer
 *        is wanted.
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

/*
 * `OZFN(^{ ... })` -- the same trick applied to one *expression* rather
 * than a whole invocation, for a callback that is not a macro argument at
 * all (#300).
 *
 * Zephyr's connection callbacks are the shape that needs it:
 *
 *     BT_CONN_CB_DEFINE(conn_callbacks) = {
 *             .connected = OZFN(^(struct bt_conn *conn, uint8_t err) { ... }),
 *             .recycled  = OZFN(^(void) { ... }),
 *     };
 *
 * The macro takes only the *name*; the callbacks sit in a designated
 * initializer after the `=`, so `OZM` has nothing to wrap. `OZFN` wraps
 * each block instead.
 *
 * **Prefer it to `OZM` where both fit.** `OZM` discards the whole
 * invocation, so what the macro declares is invisible to Clang and needs a
 * hand-written declaration (the second limit above). `OZFN` hides only the
 * block, so Clang expands the real macro and sees the real symbol -- no
 * `#ifdef __OBJC__` twin.
 *
 * **One place it is wrong and `OZM` is right:** a target macro that
 * token-pastes its callback into a symbol name. `INPUT_CALLBACK_DEFINE`
 * does (`_input_callback__##name`, with `name` defaulting to the callback),
 * so with `OZFN` the argument expands to `0` before the paste and two
 * callbacks in one file both become `_input_callback__0` -- Clang reports
 * `redefinition`, and it does so on the AST-dump path, where a truncated
 * dump silently costs ivar ownership facts. Use `OZM` there, or Zephyr's
 * own `INPUT_CALLBACK_DEFINE_NAMED` to choose the symbol yourself.
 *
 * Expands to `0` rather than to nothing, because the position it stands in
 * wants a value: a null pointer constant, which converts to any function
 * pointer type. Nothing cleverer is available -- `((blk), 0)` and
 * `((void)sizeof(blk), 0)` both have the value zero and are *not* null
 * pointer constants, so a pointer initializer rejects them
 * (`-Wint-conversion`). That is also why Clang cannot be made to check the
 * block: to reach a static initializer the expansion must be a constant,
 * and the block has to go unparsed.
 *
 * Variadic for `OZM`'s reason -- a comma at the top level of the block body
 * would otherwise split the argument list
 * (`too many arguments provided to function-like macro invocation`).
 */
#ifdef __OBJC__
#define OZFN(...) 0
#endif
