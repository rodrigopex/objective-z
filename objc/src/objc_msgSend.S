/*
 * objc_msgSend trampoline for ARM Cortex-M (Thumb-2, no FPU).
 *
 * gnustep-1.7 runtime emits calls to objc_msgSend(id self, SEL _cmd, ...)
 * instead of the GCC-style objc_msg_lookup + indirect call pattern.
 *
 * This trampoline:
 *   1. Saves r0-r3 (the first 4 args including self and _cmd)
 *   2. Calls objc_msg_lookup(r0=self, r1=_cmd) to get the IMP
 *   3. Restores r0-r3
 *   4. Tail-calls the IMP via bx, preserving all arguments
 *
 * Stack arguments beyond r0-r3 remain untouched on the stack
 * and are naturally forwarded to the IMP.
 */

	.syntax unified
	.thumb
	.text

	.global objc_msgSend
	.type   objc_msgSend, %function
	.thumb_func
objc_msgSend:
	/* Save r0-r3 and lr on the stack */
	push	{r0, r1, r2, r3, lr}

	/* Call objc_msg_lookup(self=r0, _cmd=r1) — r0,r1 already set */
	bl	objc_msg_lookup

	/* Save IMP (returned in r0) to r12 (ip) */
	mov	r12, r0

	/* Restore original r0-r3 and lr */
	pop	{r0, r1, r2, r3, lr}

	/* Tail-call the IMP — stack args pass through unchanged */
	bx	r12

	.size objc_msgSend, . - objc_msgSend
