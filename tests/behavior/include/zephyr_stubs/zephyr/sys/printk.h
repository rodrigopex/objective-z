/* Minimal Zephyr printk stub for host-side builds.
 *
 * Added so `src/OZLog.c` -- the one pure-C runtime file both backends link
 * -- can be compiled on host at all. Without it that file could only be
 * built by a real Zephyr cross-compile, which left the static backend's
 * include of the generated dispatch header unchecked (see PARITY.md on
 * OZ_BACKEND_STATIC).
 *
 * `printk` is a *prototype*, not a macro, and its definition lives in
 * `zephyr_stubs.c` for a caller to link. A macro would collide with
 * transpiled sources that declare the function themselves --
 * `samples/pool_demo/src/main.m` writes `int printk(const char *fmt, ...);`
 * so its Clang AST dump resolves without Zephyr headers -- whereas an
 * identical redeclaration of a prototype is simply legal C.
 *
 * The definition prints for real rather than discarding: OZLog's whole job
 * is formatting, and a stub that swallowed its output would let a
 * formatting bug pass unnoticed.
 */
#ifndef ZEPHYR_SYS_PRINTK_STUB_H
#define ZEPHYR_SYS_PRINTK_STUB_H

#include <stdarg.h>
#include <stdio.h>

int printk(const char *fmt, ...);

#define snprintk(buf, sz, ...)   snprintf((buf), (sz), __VA_ARGS__)
#define vsnprintk(buf, sz, f, a) vsnprintf((buf), (sz), (f), (a))

#endif /* ZEPHYR_SYS_PRINTK_STUB_H */
