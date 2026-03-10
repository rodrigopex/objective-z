/*
 * objc_msgSend trampoline for RISC-V (RV32I / RV64I).
 *
 * gnustep-2.0 runtime emits calls to objc_msgSend(id self, SEL _cmd, ...)
 * instead of the GCC-style objc_msg_lookup + indirect call pattern.
 *
 * This trampoline:
 *   1. Saves a0-a7 (the 8 argument registers) and ra
 *   2. Calls objc_msg_lookup(a0=self, a1=_cmd) to get the IMP
 *   3. Saves the IMP to t0
 *   4. Restores a0-a7 and ra
 *   5. Tail-calls the IMP via jr t0, preserving all arguments
 *
 * Stack arguments beyond the register file remain untouched and are
 * naturally forwarded to the IMP.
 *
 * Unified RV32/RV64 via __riscv_xlen preprocessor.
 */

#if __riscv_xlen == 64
#define REGBYTES 8
#define LOAD     ld
#define STORE    sd
#else
#define REGBYTES 4
#define LOAD     lw
#define STORE    sw
#endif

/* 10 slots * REGBYTES: a0-a7 (8) + ra (1) + padding (1) = 16-byte aligned */
#define FRAME_SIZE (10 * REGBYTES)

	.text
	.global objc_msgSend
	.type   objc_msgSend, @function
	.balign 4
objc_msgSend:
	/* Allocate stack frame */
	addi	sp, sp, -FRAME_SIZE

	/* Save argument registers and return address */
	STORE	ra,  (0 * REGBYTES)(sp)
	STORE	a0,  (1 * REGBYTES)(sp)
	STORE	a1,  (2 * REGBYTES)(sp)
	STORE	a2,  (3 * REGBYTES)(sp)
	STORE	a3,  (4 * REGBYTES)(sp)
	STORE	a4,  (5 * REGBYTES)(sp)
	STORE	a5,  (6 * REGBYTES)(sp)
	STORE	a6,  (7 * REGBYTES)(sp)
	STORE	a7,  (8 * REGBYTES)(sp)

	/* Call objc_msg_lookup(self=a0, _cmd=a1) — a0,a1 already set */
	call	objc_msg_lookup

	/* Save IMP (returned in a0) to t0 */
	mv	t0, a0

	/* Restore argument registers and return address */
	LOAD	ra,  (0 * REGBYTES)(sp)
	LOAD	a0,  (1 * REGBYTES)(sp)
	LOAD	a1,  (2 * REGBYTES)(sp)
	LOAD	a2,  (3 * REGBYTES)(sp)
	LOAD	a3,  (4 * REGBYTES)(sp)
	LOAD	a4,  (5 * REGBYTES)(sp)
	LOAD	a5,  (6 * REGBYTES)(sp)
	LOAD	a6,  (7 * REGBYTES)(sp)
	LOAD	a7,  (8 * REGBYTES)(sp)

	/* Deallocate stack frame */
	addi	sp, sp, FRAME_SIZE

	/* Tail-call the IMP — stack args pass through unchanged */
	jr	t0

	.size objc_msgSend, . - objc_msgSend
