/* $OpenBSD: locore.S,v 1.53 2026/09/09 16:08:18 deraadt Exp $ */
/*-
 * Copyright (c) 2012-2014 Andrew Turner
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: head/sys/arm64/arm64/locore.S 282867 2015-05-13 18:57:03Z zbb $
 */

/*
 * The ported subset of locore.S: the signal trampoline (see locore.rs). The kernel entry,
 * drop_to_el1, the boot page tables and initstack are the boot protocol's (Limine). The
 * symbols written as NAME between braces are filled in by locore.rs from the Rust
 * definitions (assym.h and <sys/syscall.h> in C).
 */

/* The signal trampoline saves and restores the floating-point registers. */
.arch_extension	fp

	.text
	.globl	sigcode
	.type	sigcode,@function
sigcode:
	sub	sp, sp, #17 * 32
	mov	x3, sp
	stp	q0, q1, [x3], #32
	stp	q2, q3, [x3], #32
	stp	q4, q5, [x3], #32
	stp	q6, q7, [x3], #32
	stp	q8, q9, [x3], #32
	stp	q10, q11, [x3], #32
	stp	q12, q13, [x3], #32
	stp	q14, q15, [x3], #32
	stp	q16, q17, [x3], #32
	stp	q18, q19, [x3], #32
	stp	q20, q21, [x3], #32
	stp	q22, q23, [x3], #32
	stp	q24, q25, [x3], #32
	stp	q26, q27, [x3], #32
	stp	q28, q29, [x3], #32
	stp	q30, q31, [x3], #32
	mrs	x4, fpsr
	mrs	x5, fpcr
	stp	w4, w5, [x3]
	blr	lr
	mov	x3, sp
	ldp	q0, q1, [x3], #32
	ldp	q2, q3, [x3], #32
	ldp	q4, q5, [x3], #32
	ldp	q6, q7, [x3], #32
	ldp	q8, q9, [x3], #32
	ldp	q10, q11, [x3], #32
	ldp	q12, q13, [x3], #32
	ldp	q14, q15, [x3], #32
	ldp	q16, q17, [x3], #32
	ldp	q18, q19, [x3], #32
	ldp	q20, q21, [x3], #32
	ldp	q22, q23, [x3], #32
	ldp	q24, q25, [x3], #32
	ldp	q26, q27, [x3], #32
	ldp	q28, q29, [x3], #32
	ldp	q30, q31, [x3], #32
	ldp	w4, w5, [x3]
	mrs	x4, fpsr
	mrs	x5, fpcr
	add	sp, sp, #17 * 32
	mov	x0, sp
	add	x0, x0, #{SF_SC}

	mov	x8, #{SYS_SIGRETURN}
	.globl sigcodecall
sigcodecall:
	svc	0
	dsb	nsh
	isb
	.globl  sigcoderet
sigcoderet:
	.size	sigcode, . - sigcode	/* END(sigcode) */
	.global esigcode
esigcode:
	/* FALLTHROUGH */
	.globl	sigfill
sigfill:
	udf	#0
esigfill:

	.data
	.globl	sigfillsiz
sigfillsiz:
	.word	esigfill - sigfill

/* Back to normal kernel code. */
.arch_extension	nofp
	.text
